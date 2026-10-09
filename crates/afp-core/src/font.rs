//! Extract font metrics from the Map Coded Font (MCF) structured field.
//!
//! MCF-2 is a sequence of repeating groups, one per local font id. Each group
//! is length-prefixed and carries triplets; we read the Resource Local
//! Identifier triplet (`0x24`) for the id and the Font Descriptor Specification
//! triplet (`0x1F`) for the vertical font size (in L-units, i.e. the point size
//! expressed at 1440 units/inch).
//!
//! Best-effort: the glyph-level character increments live in external FOCA
//! character-set resources that are usually not embedded, so advance widths
//! remain estimated. The point size — which is what visibly sizes the text — is
//! recovered here.

use crate::triplet;
use std::collections::HashMap;

/// Point size (in L-units) for one mapped local font id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct FontMetric {
    pub local_id: u8,
    pub point_size_lu: i32,
}

const TRIPLET_RESOURCE_LOCAL_ID: u8 = 0x24;
const TRIPLET_FONT_DESCRIPTOR: u8 = 0x1F;

/// Parse a Map Coded Font (format 2) payload into per-local-id font metrics.
///
/// MCF-2 is a sequence of repeating groups, each prefixed by a two-byte
/// big-endian length (which includes the length bytes). Within a group, the
/// Resource Local Identifier triplet (`04 24 <type> <id>`) gives the font's
/// local id and the Font Descriptor Specification triplet (`0x1F`) gives the
/// vertical size at payload bytes `[2..4]` (after font weight and width).
/// Sizes are in 1440 units/inch.
pub fn parse_map_coded_font(data: &[u8]) -> Vec<FontMetric> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut ordinal = 0u8;
    while pos + 1 < data.len() {
        let rgl = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        if rgl < 3 || pos + rgl > data.len() {
            break;
        }
        let group = &data[pos + 2..pos + rgl];
        let mut local_id = None;
        let mut size = None;
        for t in triplet::parse_triplets(group) {
            match t.id {
                // Resource Local Identifier: [resource-type][local-id].
                TRIPLET_RESOURCE_LOCAL_ID if t.data.len() >= 2 => local_id = Some(t.data[1]),
                // Font Descriptor Specification: vertical size after weight+width.
                TRIPLET_FONT_DESCRIPTOR if t.data.len() >= 4 => {
                    size = Some(u16::from_be_bytes([t.data[2], t.data[3]]) as i32);
                }
                _ => {}
            }
        }
        if let Some(sz) = size {
            out.push(FontMetric {
                local_id: local_id.unwrap_or(ordinal),
                point_size_lu: sz,
            });
        }
        ordinal = ordinal.wrapping_add(1);
        pos += rgl;
    }
    out
}

/// Convenience: a lookup from local font id to point size (L-units).
pub fn font_size_map(data: &[u8]) -> HashMap<u8, i32> {
    parse_map_coded_font(data)
        .into_iter()
        .map(|m| (m.local_id, m.point_size_lu))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build one MCF-2 repeating group mapping `local` -> vertical size `vsize`.
    fn group(local: u8, vsize: u16) -> Vec<u8> {
        // Resource Local Id triplet: [len=4][0x24][type=05 coded font][local]
        let rid = [0x04u8, TRIPLET_RESOURCE_LOCAL_ID, 0x05, local];
        // Font Descriptor Spec: [len=8][0x1F][weight][width][height(2)][hwidth(2)]
        let vs = vsize.to_be_bytes();
        let fds = [
            0x08,
            TRIPLET_FONT_DESCRIPTOR,
            0x05,
            0x05,
            vs[0],
            vs[1],
            vs[0],
            vs[1],
        ];
        // Two-byte repeating-group length, includes itself.
        let rgl = (2 + rid.len() + fds.len()) as u16;
        let mut g = rgl.to_be_bytes().to_vec();
        g.extend_from_slice(&rid);
        g.extend_from_slice(&fds);
        g
    }

    #[test]
    fn parses_two_font_sizes() {
        let mut data = group(1, 200); // 10pt at 1440 upi
        data.extend(group(2, 280)); // 14pt
        let map = font_size_map(&data);
        assert_eq!(map.get(&1), Some(&200));
        assert_eq!(map.get(&2), Some(&280));
    }

    #[test]
    fn malformed_group_stops_without_panic() {
        // A repeating-group length that overruns the buffer.
        let data = [0x40u8, 0x03, 0x24, 0x01];
        assert!(parse_map_coded_font(&data).is_empty());
    }

    #[test]
    fn empty_is_empty() {
        assert!(parse_map_coded_font(&[]).is_empty());
    }
}
