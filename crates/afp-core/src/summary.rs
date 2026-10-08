//! A compact, display-ready summary of a parsed document.

use crate::names::{self, Kind};
use crate::tree::{Document, Node};

/// Headline counts for the whole document.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Summary {
    /// Number of Begin Page (BPG) fields.
    pub pages: usize,
    /// Total structured fields (excluding the synthetic root).
    pub fields: usize,
    /// Number of Begin Resource (BR) wrappers.
    pub resources: usize,
    /// File size in bytes.
    pub byte_size: usize,
    /// Number of non-fatal problems recorded during parsing.
    pub problems: usize,
}

/// Begin Page category byte.
const CAT_PAGE: u8 = 0xAF;
/// Begin Resource category byte.
const CAT_RESOURCE: u8 = 0xCE;

impl Document {
    /// Compute a headline summary. `byte_size` is the original file length.
    pub fn summary(&self, byte_size: usize) -> Summary {
        let mut pages = 0;
        let mut resources = 0;
        walk(&self.root, &mut |n| {
            if names::classify(n.sfid) == Kind::Begin {
                match names::category(n.sfid) {
                    CAT_PAGE => pages += 1,
                    CAT_RESOURCE => resources += 1,
                    _ => {}
                }
            }
        });
        Summary {
            pages,
            fields: self.field_count,
            resources,
            byte_size,
            problems: self.problems.len(),
        }
    }
}

/// Visit every node except the synthetic root.
fn walk(node: &Node, f: &mut impl FnMut(&Node)) {
    for child in &node.children {
        f(child);
        walk(child, f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::StreamBuilder;

    const BDT: [u8; 3] = [0xD3, 0xA8, 0xA8];
    const EDT: [u8; 3] = [0xD3, 0xA9, 0xA8];
    const BPG: [u8; 3] = [0xD3, 0xA8, 0xAF];
    const EPG: [u8; 3] = [0xD3, 0xA9, 0xAF];
    const BRG: [u8; 3] = [0xD3, 0xA8, 0xC6];
    const ERG: [u8; 3] = [0xD3, 0xA9, 0xC6];
    const BR: [u8; 3] = [0xD3, 0xA8, 0xCE];
    const ER: [u8; 3] = [0xD3, 0xA9, 0xCE];
    const BIM: [u8; 3] = [0xD3, 0xA8, 0xFB];
    const EIM: [u8; 3] = [0xD3, 0xA9, 0xFB];
    const PTX: [u8; 3] = [0xD3, 0xEE, 0x9B];

    #[test]
    fn counts_pages_fields_resources() {
        let bytes = StreamBuilder::new()
            .begin(BDT)
            .begin(BRG)
            .begin(BR)
            .begin(BIM)
            .end(EIM)
            .end(ER)
            .end(ERG)
            .begin(BPG)
            .other(PTX, &[0xAB])
            .end(EPG)
            .begin(BPG)
            .end(EPG)
            .end(EDT)
            .build();
        let doc = Document::parse(&bytes).unwrap();
        let s = doc.summary(bytes.len());
        assert_eq!(s.pages, 2);
        assert_eq!(s.resources, 1);
        assert_eq!(s.byte_size, bytes.len());
        assert_eq!(s.problems, 0);
        // 13 structured fields emitted above.
        assert_eq!(s.fields, 13);
    }
}
