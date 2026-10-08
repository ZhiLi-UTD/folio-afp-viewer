//! Extract preview-ready image bytes from an IOCA image object.
//!
//! Phase 1 targets the common FS11 case where a complete JPEG is embedded in
//! the image object's data: we locate it by its SOI (`FF D8 FF`) and EOI
//! (`FF D9`) markers and hand the bytes to the webview, which renders JPEG
//! natively — so no image-decoding dependency is pulled in. Other compressions
//! (FS10 G4/MMR, uncompressed IM) report [`ImageFormat::Unsupported`].

use crate::tree::{Document, Node};

/// Detected image encoding of an extracted image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ImageFormat {
    /// A full JPEG was found and returned.
    Jpeg,
    /// The image uses a compression Folio cannot preview yet.
    Unsupported,
}

/// Result of attempting to extract a previewable image from an object.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExtractedImage {
    pub format: ImageFormat,
    /// Image bytes (empty when `format` is `Unsupported`).
    pub bytes: Vec<u8>,
}

impl Document {
    /// Extract a previewable image from the object at `node_index` (the BR or
    /// image-object node). Returns `None` if the node does not exist.
    pub fn extract_image(&self, node_index: usize, buf: &[u8]) -> Option<ExtractedImage> {
        let node = self.find(node_index)?;
        Some(extract_image(node, buf))
    }
}

/// Extract a previewable image from `node`.
///
/// Concatenates the *data* bytes of the node and its descendants in document
/// order (so the structured-field headers are excluded) and scans the result
/// for a JPEG. Excluding headers is what makes multi-record IOCA images — where
/// a JPEG is split across several Image Picture Data records — reassemble
/// correctly instead of being spliced with `0x5A` introducers.
pub fn extract_image(node: &Node, buf: &[u8]) -> ExtractedImage {
    let data = collect_data(node, buf);
    match find_jpeg(&data) {
        Some(jpeg) => ExtractedImage {
            format: ImageFormat::Jpeg,
            bytes: jpeg.to_vec(),
        },
        None => ExtractedImage {
            format: ImageFormat::Unsupported,
            bytes: Vec::new(),
        },
    }
}

/// Concatenate the data-range bytes of `node` and all descendants, in order.
fn collect_data(node: &Node, buf: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    push_data(node, buf, &mut out);
    out
}

fn push_data(node: &Node, buf: &[u8], out: &mut Vec<u8>) {
    let start = node.data_range.start.min(buf.len());
    let end = node.data_range.end.min(buf.len());
    if start < end {
        out.extend_from_slice(&buf[start..end]);
    }
    for child in &node.children {
        push_data(child, buf, out);
    }
}

/// Find a complete JPEG within `data` by walking its marker segments from SOI
/// (`FF D8`) to the matching EOI (`FF D9`). Parsing segment lengths — and
/// scanning entropy-coded data past stuffed `FF 00` and restart markers — means
/// an `FF D9` byte embedded inside compressed data is not mistaken for the end.
/// Fully bounds-checked; returns `None` on malformed input rather than panicking.
fn find_jpeg(data: &[u8]) -> Option<&[u8]> {
    let soi = data.windows(2).position(|w| w == [0xFF, 0xD8])?;
    let mut i = soi + 2;
    while i + 1 < data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        // Collapse fill bytes (runs of 0xFF) before the marker code.
        let mut m = i + 1;
        while m < data.len() && data[m] == 0xFF {
            m += 1;
        }
        if m >= data.len() {
            break;
        }
        let marker = data[m];
        let after = m + 1;
        match marker {
            0xD9 => return Some(&data[soi..after]), // EOI
            // Standalone markers (TEM, restart) carry no length.
            0x01 | 0xD0..=0xD7 => i = after,
            0xDA => {
                // Start of Scan: skip its header, then scan entropy data for the
                // next real marker (FF followed by non-0x00, non-restart).
                let len = seg_len(data, after)?;
                let mut j = after + len;
                while j + 1 < data.len() {
                    if data[j] == 0xFF {
                        let n = data[j + 1];
                        if n != 0x00 && !(0xD0..=0xD7).contains(&n) {
                            break;
                        }
                    }
                    j += 1;
                }
                i = j;
            }
            // All other markers carry a 2-byte segment length.
            _ => i = after + seg_len(data, after)?,
        }
    }
    None
}

/// Length of a marker segment whose 2-byte big-endian length starts at `at`
/// (the length counts itself). Returns `None` if out of bounds.
fn seg_len(data: &[u8], at: usize) -> Option<usize> {
    if at + 1 < data.len() {
        Some((u16::from_be_bytes([data[at], data[at + 1]]) as usize).max(2))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use afpgen::sample_jpeg::SAMPLE_JPEG;

    #[test]
    fn extracts_embedded_jpeg_from_image_resource() {
        let bytes = afpgen::with_image();
        let doc = Document::parse(&bytes).unwrap();
        let res = doc.resources(&bytes);
        let img = doc.extract_image(res[0].node_index, &bytes).unwrap();
        assert_eq!(img.format, ImageFormat::Jpeg);
        assert_eq!(img.bytes, SAMPLE_JPEG);
    }

    #[test]
    fn reassembles_jpeg_split_across_records() {
        // Real IOCA splits image data across multiple IPD records; the
        // extractor must stitch the data bytes back without header bytes.
        let bytes = afpgen::with_image_split();
        let doc = Document::parse(&bytes).unwrap();
        let res = doc.resources(&bytes);
        let img = doc.extract_image(res[0].node_index, &bytes).unwrap();
        assert_eq!(img.format, ImageFormat::Jpeg);
        assert_eq!(img.bytes, SAMPLE_JPEG);
    }

    #[test]
    fn non_image_object_is_unsupported() {
        let bytes = afpgen::simple();
        let doc = Document::parse(&bytes).unwrap();
        // The root has no JPEG anywhere.
        let img = extract_image(&doc.root, &bytes);
        assert_eq!(img.format, ImageFormat::Unsupported);
        assert!(img.bytes.is_empty());
    }

    #[test]
    fn soi_without_eoi_is_unsupported() {
        // A truncated JPEG (SOI present, no EOI) must not be returned as valid.
        let data = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        assert!(find_jpeg(&data).is_none());
    }

    #[test]
    fn embedded_ff_d9_in_segment_is_not_mistaken_for_eoi() {
        // APP0 payload contains FF D9; the real EOI is after the scan data.
        let data = [
            0xFF, 0xD8, // SOI
            0xFF, 0xE0, 0x00, 0x06, 0xAA, 0xFF, 0xD9, 0xBB, // APP0 w/ embedded FF D9
            0xFF, 0xDA, 0x00, 0x03, 0x00, // SOS header
            0x11, 0x22, // entropy
            0xFF, 0xD9, // real EOI
        ];
        let jpeg = find_jpeg(&data).expect("should find full jpeg");
        assert_eq!(jpeg.len(), data.len()); // full image, not truncated at embedded FF D9
    }
}
