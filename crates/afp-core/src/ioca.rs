//! Extract preview-ready image bytes from an IOCA image object.
//!
//! Phase 1 targets the common FS11 case where a complete JPEG is embedded in
//! the image object's data: we locate it by its SOI (`FF D8 FF`) and EOI
//! (`FF D9`) markers and hand the bytes to the webview, which renders JPEG
//! natively — so no image-decoding dependency is pulled in. Other compressions
//! (FS10 G4/MMR, uncompressed IM) report [`ImageFormat::Unsupported`].

use crate::tree::{Document, Node};
use std::ops::Range;

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

/// Extract a previewable image from `node` by scanning its subtree bytes.
pub fn extract_image(node: &Node, buf: &[u8]) -> ExtractedImage {
    let span = subtree_span(node);
    let region = &buf[span.start.min(buf.len())..span.end.min(buf.len())];
    match find_jpeg(region) {
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

/// Byte span covering a node and all of its descendants.
fn subtree_span(node: &Node) -> Range<usize> {
    let mut start = node.record_range.start;
    let mut end = node.record_range.end;
    visit(node, &mut |n| {
        start = start.min(n.record_range.start);
        end = end.max(n.record_range.end);
    });
    start..end
}

fn visit(node: &Node, f: &mut impl FnMut(&Node)) {
    for child in &node.children {
        f(child);
        visit(child, f);
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
    fn non_image_object_is_unsupported() {
        let bytes = afpgen::simple();
        let doc = Document::parse(&bytes).unwrap();
        // The root has no JPEG anywhere.
        let img = extract_image(&doc.root, &bytes);
        assert_eq!(img.format, ImageFormat::Unsupported);
        assert!(img.bytes.is_empty());
    }
}
