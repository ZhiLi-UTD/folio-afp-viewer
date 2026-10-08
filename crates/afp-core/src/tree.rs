//! Build a nesting tree from the flat structured-field sequence.
//!
//! Begin fields open a scope, End fields close it; matching is by the category
//! (third SFID byte). The builder is resilient: mismatched or unclosed scopes
//! are recorded as [`Problem`]s rather than aborting, and every field still
//! appears in the tree.

use crate::names::{self, Kind};
use crate::sf::{self, ParseError, StructuredField};
use std::ops::Range;

/// One node in the structured-field tree.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Node {
    /// Stream-order index of the field (unique; used for selection/search).
    pub index: usize,
    pub sfid: [u8; 3],
    /// Human-readable name, or `None` when the SFID is unknown.
    pub name: Option<&'static str>,
    pub kind: Kind,
    pub record_range: Range<usize>,
    pub data_range: Range<usize>,
    pub children: Vec<Node>,
}

/// A non-fatal issue encountered while building the tree.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Problem {
    pub message: String,
    /// Byte offset of the offending record.
    pub at: usize,
}

/// A parsed AFP document: the field tree plus any problems found.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Document {
    /// Synthetic root whose children are the top-level fields.
    pub root: Node,
    pub problems: Vec<Problem>,
}

/// An open Begin scope awaiting its matching End.
struct Open {
    field: StructuredField,
    index: usize,
    children: Vec<Node>,
}

fn leaf(field: &StructuredField, index: usize) -> Node {
    Node {
        index,
        sfid: field.sfid,
        name: names::name(field.sfid),
        kind: names::classify(field.sfid),
        record_range: field.record_range.clone(),
        data_range: field.data_range.clone(),
        children: Vec::new(),
    }
}

fn close(open: Open) -> Node {
    Node {
        index: open.index,
        sfid: open.field.sfid,
        name: names::name(open.field.sfid),
        kind: Kind::Begin,
        record_range: open.field.record_range.clone(),
        data_range: open.field.data_range.clone(),
        children: open.children,
    }
}

impl Document {
    /// Parse an AFP byte stream into a document tree.
    ///
    /// Returns [`ParseError`] only for stream-level framing failures (not AFP,
    /// or structurally truncated). Structured-field nesting issues are captured
    /// in [`Document::problems`].
    pub fn parse(buf: &[u8]) -> Result<Document, ParseError> {
        let fields = sf::parse_fields(buf)?;
        let mut problems = Vec::new();

        // Root children live at the bottom of the stack; each Begin pushes a frame.
        let mut root_children: Vec<Node> = Vec::new();
        let mut stack: Vec<Open> = Vec::new();

        for (index, field) in fields.into_iter().enumerate() {
            match names::classify(field.sfid) {
                Kind::Begin => {
                    stack.push(Open {
                        index,
                        field,
                        children: Vec::new(),
                    });
                }
                Kind::End => match stack.pop() {
                    None => {
                        problems.push(Problem {
                            message: format!(
                                "End field {} has no matching Begin",
                                sfid_hex(field.sfid)
                            ),
                            at: field.record_range.start,
                        });
                        // Still show it, as a leaf at the top level.
                        root_children.push(leaf(&field, index));
                    }
                    Some(open) => {
                        if names::category(open.field.sfid) != names::category(field.sfid) {
                            problems.push(Problem {
                                message: format!(
                                    "End {} does not match open Begin {}",
                                    sfid_hex(field.sfid),
                                    sfid_hex(open.field.sfid)
                                ),
                                at: field.record_range.start,
                            });
                        }
                        let node = close(open);
                        push_child(&mut stack, &mut root_children, node);
                    }
                },
                Kind::Other => {
                    let node = leaf(&field, index);
                    push_child(&mut stack, &mut root_children, node);
                }
            }
        }

        // Any still-open scopes are unclosed; close them innermost-first.
        while let Some(open) = stack.pop() {
            problems.push(Problem {
                message: format!("Begin {} was never closed", sfid_hex(open.field.sfid)),
                at: open.field.record_range.start,
            });
            let node = close(open);
            push_child(&mut stack, &mut root_children, node);
        }

        Ok(Document {
            root: Node {
                index: usize::MAX,
                sfid: [0, 0, 0],
                name: Some("Document Root"),
                kind: Kind::Other,
                record_range: 0..buf.len(),
                data_range: 0..buf.len(),
                children: root_children,
            },
            problems,
        })
    }
}

/// Append `node` to the children of the current open scope, or to root.
fn push_child(stack: &mut [Open], root_children: &mut Vec<Node>, node: Node) {
    match stack.last_mut() {
        Some(open) => open.children.push(node),
        None => root_children.push(node),
    }
}

fn sfid_hex(sfid: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", sfid[0], sfid[1], sfid[2])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::StreamBuilder;

    const BDT: [u8; 3] = [0xD3, 0xA8, 0xA8];
    const EDT: [u8; 3] = [0xD3, 0xA9, 0xA8];
    const BPG: [u8; 3] = [0xD3, 0xA8, 0xAF];
    const EPG: [u8; 3] = [0xD3, 0xA9, 0xAF];
    const PTX: [u8; 3] = [0xD3, 0xEE, 0x9B];

    #[test]
    fn builds_nested_tree() {
        let bytes = StreamBuilder::new()
            .begin(BDT)
            .begin(BPG)
            .other(PTX, &[0xAB])
            .end(EPG)
            .end(EDT)
            .build();
        let doc = Document::parse(&bytes).unwrap();
        assert!(doc.problems.is_empty(), "problems: {:?}", doc.problems);
        assert_eq!(doc.root.children.len(), 1); // BDT
        let bdt = &doc.root.children[0];
        assert_eq!(bdt.sfid, BDT);
        assert_eq!(bdt.children.len(), 1); // BPG
        assert_eq!(bdt.children[0].children.len(), 1); // PTX leaf
        assert_eq!(bdt.children[0].children[0].sfid, PTX);
    }

    #[test]
    fn mismatched_end_is_recorded_as_problem() {
        let bytes = StreamBuilder::new().begin(BPG).end(EDT).build();
        let doc = Document::parse(&bytes).unwrap();
        assert!(!doc.problems.is_empty());
    }

    #[test]
    fn unclosed_begin_is_recorded_as_problem() {
        let bytes = StreamBuilder::new().begin(BDT).begin(BPG).end(EPG).build();
        let doc = Document::parse(&bytes).unwrap();
        assert_eq!(doc.problems.len(), 1);
        assert!(doc.problems[0].message.contains("never closed"));
        // BDT still present with BPG nested inside.
        assert_eq!(doc.root.children.len(), 1);
        assert_eq!(doc.root.children[0].children.len(), 1);
    }

    #[test]
    fn stray_end_is_recorded_and_shown() {
        let bytes = StreamBuilder::new().end(EPG).build();
        let doc = Document::parse(&bytes).unwrap();
        assert_eq!(doc.problems.len(), 1);
        assert_eq!(doc.root.children.len(), 1);
    }
}
