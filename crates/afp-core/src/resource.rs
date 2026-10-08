//! Enumerate interchange resources (images, page segments, overlays, fonts)
//! carried inside Begin Resource (BR) wrappers.
//!
//! A resource's *kind* is taken from the category of the first nested Begin
//! field inside its BR wrapper. Its *name* is the Fully Qualified Name triplet
//! (id `0x02`) on the BR field, decoded from EBCDIC.

use crate::names::{self, Kind};
use crate::tree::{Document, Node};
use crate::triplet;
use std::ops::Range;

/// What sort of object a resource contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ResourceKind {
    ImageObject,
    PageSegment,
    Overlay,
    PresentationText,
    Other,
}

/// One interchange resource found in the document.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Resource {
    /// Resource name from the BR Fully Qualified Name triplet, if present.
    pub name: Option<String>,
    pub kind: ResourceKind,
    /// `index` of the BR node (resolve with [`Document::find`]).
    pub node_index: usize,
    /// Byte span of the BR wrapper record (not the whole subtree).
    pub record_range: Range<usize>,
}

/// Begin Resource category byte.
const CAT_RESOURCE: u8 = 0xCE;

impl Document {
    /// All interchange resources in document order.
    pub fn resources(&self, buf: &[u8]) -> Vec<Resource> {
        let mut out = Vec::new();
        collect(&self.root, buf, &mut out);
        out
    }

    /// Find a node by its stream-order `index`.
    pub fn find(&self, index: usize) -> Option<&Node> {
        find_in(&self.root, index)
    }
}

fn find_in<'a>(node: &'a Node, index: usize) -> Option<&'a Node> {
    if node.index == index {
        return Some(node);
    }
    node.children.iter().find_map(|c| find_in(c, index))
}

fn collect(node: &Node, buf: &[u8], out: &mut Vec<Resource>) {
    for child in &node.children {
        if names::classify(child.sfid) == Kind::Begin && names::category(child.sfid) == CAT_RESOURCE
        {
            out.push(resource_from_br(child, buf));
        }
        collect(child, buf, out);
    }
}

fn resource_from_br(br: &Node, buf: &[u8]) -> Resource {
    let name = fqn_name(&buf[br.data_range.clone()]);
    Resource {
        name,
        kind: kind_of(br),
        node_index: br.index,
        record_range: br.record_range.clone(),
    }
}

fn kind_of(br: &Node) -> ResourceKind {
    let Some(inner) = br
        .children
        .iter()
        .find(|c| names::classify(c.sfid) == Kind::Begin)
    else {
        return ResourceKind::Other;
    };
    match names::category(inner.sfid) {
        0xFB => ResourceKind::ImageObject,
        0x5F => ResourceKind::PageSegment,
        0xDF => ResourceKind::Overlay,
        0x9B => ResourceKind::PresentationText,
        _ => ResourceKind::Other,
    }
}

/// Extract and decode the Fully Qualified Name (triplet id `0x02`).
/// FQN data is `[type][format][name...]`; the name is EBCDIC.
fn fqn_name(data: &[u8]) -> Option<String> {
    for t in triplet::parse_triplets(data) {
        if t.id == 0x02 && t.data.len() > 2 {
            return Some(decode_ebcdic(&t.data[2..]).trim().to_string());
        }
    }
    None
}

/// Decode the subset of EBCDIC (code page 500) that appears in AFP resource
/// names: letters, digits, space, and common punctuation. Unknown bytes become '.'.
pub fn decode_ebcdic(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| ebcdic_char(b)).collect()
}

fn ebcdic_char(b: u8) -> char {
    match b {
        0x40 => ' ',
        0x4B => '.',
        0x60 => '-',
        0x61 => '/',
        0x6D => '_',
        0x81..=0x89 => (b'a' + (b - 0x81)) as char,
        0x91..=0x99 => (b'j' + (b - 0x91)) as char,
        0xA2..=0xA9 => (b's' + (b - 0xA2)) as char,
        0xC1..=0xC9 => (b'A' + (b - 0xC1)) as char,
        0xD1..=0xD9 => (b'J' + (b - 0xD1)) as char,
        0xE2..=0xE9 => (b'S' + (b - 0xE2)) as char,
        0xF0..=0xF9 => (b'0' + (b - 0xF0)) as char,
        _ => '.',
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::StreamBuilder;

    const BRG: [u8; 3] = [0xD3, 0xA8, 0xC6];
    const ERG: [u8; 3] = [0xD3, 0xA9, 0xC6];
    const BR: [u8; 3] = [0xD3, 0xA8, 0xCE];
    const ER: [u8; 3] = [0xD3, 0xA9, 0xCE];
    const BIM: [u8; 3] = [0xD3, 0xA8, 0xFB];
    const EIM: [u8; 3] = [0xD3, 0xA9, 0xFB];

    #[test]
    fn decodes_ebcdic_name() {
        // "IMG1" in EBCDIC: I=0xC9 M=0xD4? -> M is 0xD4 which is J-range? J=0xD1..R=0xD9 => M=0xD1+3=0xD4. G=0xC7. 1=0xF1
        assert_eq!(decode_ebcdic(&[0xC9, 0xD4, 0xC7, 0xF1]), "IMG1");
    }

    #[test]
    fn enumerates_image_resource_with_name() {
        // BR data: FQN triplet (id 0x02): len, id, type, format, name("PIC")
        // name PIC -> P=0xD7 I=0xC9 C=0xC3
        let fqn = [0x07u8, 0x02, 0x00, 0x00, 0xD7, 0xC9, 0xC3];
        let bytes = StreamBuilder::new()
            .begin(BRG)
            .field(BR, &fqn)
            .begin(BIM)
            .end(EIM)
            .end(ER)
            .end(ERG)
            .build();
        let doc = Document::parse(&bytes).unwrap();
        let res = doc.resources(&bytes);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].kind, ResourceKind::ImageObject);
        assert_eq!(res[0].name.as_deref(), Some("PIC"));
    }

    #[test]
    fn resource_without_fqn_has_no_name() {
        let bytes = StreamBuilder::new()
            .begin(BRG)
            .begin(BR)
            .begin(BIM)
            .end(EIM)
            .end(ER)
            .end(ERG)
            .build();
        let doc = Document::parse(&bytes).unwrap();
        let res = doc.resources(&bytes);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].name, None);
        assert_eq!(res[0].kind, ResourceKind::ImageObject);
    }
}
