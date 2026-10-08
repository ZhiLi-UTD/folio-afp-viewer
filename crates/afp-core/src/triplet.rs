//! Generic triplet (TLV) decoding.
//!
//! Many structured fields carry a sequence of *triplets*, each encoded as
//! `[length][id][data...]` where `length` includes the length byte and the id
//! byte (so `data` is `length - 2` bytes). Decoding is resilient: a malformed
//! length stops the scan and the consumed triplets are returned.

/// One decoded triplet.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Triplet {
    pub id: u8,
    pub data: Vec<u8>,
}

/// Decode a sequence of triplets from `bytes`, stopping at the first malformed
/// length (a length of 0 or 1, or one that runs past the end).
pub fn parse_triplets(bytes: &[u8]) -> Vec<Triplet> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let len = bytes[pos] as usize;
        // length counts itself + id, so the smallest meaningful triplet is 2.
        if len < 2 || pos + len > bytes.len() {
            break;
        }
        let id = bytes[pos + 1];
        let data = bytes[pos + 2..pos + len].to_vec();
        out.push(Triplet { id, data });
        pos += len;
    }
    out
}

/// Human-readable name for a known triplet id, or `None`.
///
/// Verified subset from the MO:DCA Reference; extend append-only.
pub fn triplet_name(id: u8) -> Option<&'static str> {
    Some(match id {
        0x01 => "Coded Graphic Character Set Global Identifier",
        0x02 => "Fully Qualified Name",
        0x04 => "Mapping Option",
        0x10 => "Object Classification",
        0x18 => "MO:DCA Interchange Set",
        0x1F => "Font Descriptor Specification",
        0x21 => "Object Function Set Specification",
        0x24 => "Resource Local Identifier",
        0x26 => "Resource Section Number",
        0x36 => "Object Byte Extent",
        0x4B => "Object Offset",
        0x62 => "Object Area Size",
        0x6C => "Presentation Space Reset Mixing",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_triplet_sequence() {
        // len=4,id=0x02,data=[0xAA,0xBB] ; len=3,id=0x01,data=[0xCC]
        let bytes = [0x04, 0x02, 0xAA, 0xBB, 0x03, 0x01, 0xCC];
        let t = parse_triplets(&bytes);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0], Triplet { id: 0x02, data: vec![0xAA, 0xBB] });
        assert_eq!(t[1], Triplet { id: 0x01, data: vec![0xCC] });
    }

    #[test]
    fn stops_on_bad_length_without_panicking() {
        // second triplet claims len 9 but only 2 bytes remain
        let bytes = [0x03, 0x01, 0xCC, 0x09, 0x02];
        let t = parse_triplets(&bytes);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].id, 0x01);
    }

    #[test]
    fn empty_is_empty() {
        assert!(parse_triplets(&[]).is_empty());
    }

    #[test]
    fn zero_length_stops() {
        assert!(parse_triplets(&[0x00, 0x02]).is_empty());
    }

    #[test]
    fn known_triplet_name() {
        assert_eq!(triplet_name(0x02), Some("Fully Qualified Name"));
        assert_eq!(triplet_name(0xFF), None);
    }
}
