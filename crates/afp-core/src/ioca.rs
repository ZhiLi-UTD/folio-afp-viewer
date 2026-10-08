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

/// Find a complete JPEG (SOI `FF D8 FF` .. EOI `FF D9`) within `data`.
fn find_jpeg(data: &[u8]) -> Option<&[u8]> {
    let soi = data
        .windows(3)
        .position(|w| w == [0xFF, 0xD8, 0xFF])?;
    // EOI after the SOI.
    let eoi_rel = data[soi + 2..]
        .windows(2)
        .position(|w| w == [0xFF, 0xD9])?;
    let end = soi + 2 + eoi_rel + 2; // inclusive of FF D9
    Some(&data[soi..end])
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
}
