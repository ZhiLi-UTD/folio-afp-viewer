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

fn find_in(node: &Node, index: usize) -> Option<&Node> {
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

/// Decode EBCDIC text using code page 500 (International), the common AFP
/// interchange default. Covers the full printable set — letters, digits,
/// punctuation, and Latin-1 accented characters. Control codes and unassigned
/// bytes render as '.'. (Per-object code pages from the CGCSGID triplet are
/// future work; CP500 is a sound default.)
pub fn decode_ebcdic(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| ebcdic_char(b)).collect()
}

fn ebcdic_char(b: u8) -> char {
    match b {
        // Invariant Latin letters and digits (regular ranges).
        0x81..=0x89 => (b'a' + (b - 0x81)) as char,
        0x91..=0x99 => (b'j' + (b - 0x91)) as char,
        0xA2..=0xA9 => (b's' + (b - 0xA2)) as char,
        0xC1..=0xC9 => (b'A' + (b - 0xC1)) as char,
        0xD1..=0xD9 => (b'J' + (b - 0xD1)) as char,
        0xE2..=0xE9 => (b'S' + (b - 0xE2)) as char,
        0xF0..=0xF9 => (b'0' + (b - 0xF0)) as char,
        // Space and punctuation (CP500 positions).
        0x40 => ' ',
        0x4A => '[',
        0x4B => '.',
        0x4C => '<',
        0x4D => '(',
        0x4E => '+',
        0x4F => '!',
        0x50 => '&',
        0x5A => ']',
        0x5B => '$',
        0x5C => '*',
        0x5D => ')',
        0x5E => ';',
        0x5F => '^',
        0x60 => '-',
        0x61 => '/',
        0x6A => '¦',
        0x6B => ',',
        0x6C => '%',
        0x6D => '_',
        0x6E => '>',
        0x6F => '?',
        0x79 => '`',
        0x7A => ':',
        0x7B => '#',
        0x7C => '@',
        0x7D => '\'',
        0x7E => '=',
        0x7F => '"',
        0xA1 => '~',
        0xB0 => '¢',
        0xB1 => '£',
        0xB2 => '¥',
        0xBA => '¬',
        0xBB => '|',
        0xC0 => '{',
        0xD0 => '}',
        0xE0 => '\\',
        // Typographic and symbol characters (CP500 positions).
        0x8A => '«',
        0x8B => '»',
        0x8F => '±',
        0x90 => '°',
        0x9A => 'ª',
        0x9B => 'º',
        0x9D => '¸',
        0x9F => '¤',
        0xA0 => 'µ',
        0xAA => '¡',
        0xAB => '¿',
        0xAF => '®',
        0xB3 => '·',
        0xB4 => '©',
        0xB5 => '§',
        0xB6 => '¶',
        0xB7 => '¼',
        0xB8 => '½',
        0xB9 => '¾',
        0xBC => '¯',
        0xBD => '¨',
        0xBE => '´',
        0xBF => '×',
        0xDA => '¹',
        0xE1 => '÷',
        0xEA => '²',
        0xFA => '³',
        // Latin-1 accented letters (CP500 positions).
        0x42 => 'â',
        0x43 => 'ä',
        0x44 => 'à',
        0x45 => 'á',
        0x46 => 'ã',
        0x47 => 'å',
        0x48 => 'ç',
        0x49 => 'ñ',
        0x51 => 'é',
        0x52 => 'ê',
        0x53 => 'ë',
        0x54 => 'è',
        0x55 => 'í',
        0x56 => 'î',
        0x57 => 'ï',
        0x58 => 'ì',
        0x59 => 'ß',
        0x62 => 'Â',
        0x63 => 'Ä',
        0x64 => 'À',
        0x65 => 'Á',
        0x66 => 'Ã',
        0x67 => 'Å',
        0x68 => 'Ç',
        0x69 => 'Ñ',
        0x70 => 'ø',
        0x71 => 'É',
        0x72 => 'Ê',
        0x73 => 'Ë',
        0x74 => 'È',
        0x75 => 'Í',
        0x76 => 'Î',
        0x77 => 'Ï',
        0x78 => 'Ì',
        0x80 => 'Ø',
        0x8C => 'ð',
        0x8D => 'ý',
        0x8E => 'þ',
        0x9C => 'æ',
        0x9E => 'Æ',
        0xAC => 'Ð',
        0xAD => 'Ý',
        0xAE => 'Þ',
        0xCB => 'ô',
        0xCC => 'ö',
        0xCD => 'ò',
        0xCE => 'ó',
        0xCF => 'õ',
        0xDB => 'û',
        0xDC => 'ü',
        0xDD => 'ù',
        0xDE => 'ú',
        0xDF => 'ÿ',
        0xEB => 'Ô',
        0xEC => 'Ö',
        0xED => 'Ò',
        0xEE => 'Ó',
        0xEF => 'Õ',
        0xFB => 'Û',
        0xFC => 'Ü',
        0xFD => 'Ù',
        0xFE => 'Ú',
        // Control codes and unassigned bytes.
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
    fn decodes_full_cp500_punctuation() {
        // "( ) , : & $ @ % = -" — common statement characters.
        assert_eq!(
            decode_ebcdic(&[0x4D, 0x5D, 0x6B, 0x7A, 0x50, 0x5B, 0x7C, 0x6C, 0x7E, 0x60]),
            "(),:&$@%=-"
        );
    }

    #[test]
    fn decodes_accented_latin() {
        // é ê è ç ñ ü
        assert_eq!(decode_ebcdic(&[0x51, 0x52, 0x54, 0x48, 0x49, 0xDC]), "éêèçñü");
    }

    #[test]
    fn decodes_cp500_symbols() {
        // © ° ½ § × ÷ — complete-table symbols.
        assert_eq!(decode_ebcdic(&[0xB4, 0x90, 0xB8, 0xB5, 0xBF, 0xE1]), "©°½§×÷");
    }

    #[test]
    fn control_bytes_become_placeholder() {
        assert_eq!(decode_ebcdic(&[0x00, 0x05, 0x25]), "...");
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
