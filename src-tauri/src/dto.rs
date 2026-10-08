//! Serializable view-model sent to the webview.
//!
//! Kept separate from command wiring so the mapping is unit-testable without a
//! running Tauri app. SFIDs become hex strings and enums become labels so the
//! front-end needs no decoding logic.

use afp_core::ioca::ImageFormat;
use afp_core::names::Kind;
use afp_core::tree::Node;
use afp_core::{Document, ExtractedImage, Resource, ResourceKind, Summary};
use serde::Serialize;

fn hex(sfid: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", sfid[0], sfid[1], sfid[2])
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Begin => "Begin",
        Kind::End => "End",
        Kind::Other => "Other",
    }
}

fn resource_kind_label(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::ImageObject => "Image",
        ResourceKind::PageSegment => "Page Segment",
        ResourceKind::Overlay => "Overlay",
        ResourceKind::PresentationText => "Presentation Text",
        ResourceKind::Other => "Other",
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDto {
    /// Stream-order index; the synthetic root uses `null`.
    pub index: Option<usize>,
    pub sfid: String,
    /// Decoded name, or `Unknown (HEX)` when unrecognised.
    pub name: String,
    pub known: bool,
    pub kind: &'static str,
    pub start: usize,
    pub end: usize,
    pub data_start: usize,
    pub data_end: usize,
    pub children: Vec<NodeDto>,
}

impl NodeDto {
    fn from_node(n: &Node) -> NodeDto {
        let known = n.name.is_some();
        NodeDto {
            index: (n.index != usize::MAX).then_some(n.index),
            sfid: hex(n.sfid),
            name: n
                .name
                .map(str::to_string)
                .unwrap_or_else(|| format!("Unknown ({})", hex(n.sfid))),
            known,
            kind: kind_label(n.kind),
            start: n.record_range.start,
            end: n.record_range.end,
            data_start: n.data_range.start,
            data_end: n.data_range.end,
            children: n.children.iter().map(NodeDto::from_node).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceDto {
    pub name: Option<String>,
    pub kind: &'static str,
    pub node_index: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryDto {
    pub pages: usize,
    pub fields: usize,
    pub resources: usize,
    pub byte_size: usize,
    pub problems: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemDto {
    pub message: String,
    pub at: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDto {
    pub doc_id: String,
    pub file_name: String,
    pub root: NodeDto,
    pub summary: SummaryDto,
    pub resources: Vec<ResourceDto>,
    pub problems: Vec<ProblemDto>,
}

/// Build the full view-model for an opened document.
pub fn to_document_dto(
    doc: &Document,
    buf: &[u8],
    doc_id: String,
    file_name: String,
) -> DocumentDto {
    let summary = doc.summary(buf.len());
    DocumentDto {
        doc_id,
        file_name,
        root: NodeDto::from_node(&doc.root),
        summary: summary_dto(&summary),
        resources: doc
            .resources(buf)
            .iter()
            .map(resource_dto)
            .collect(),
        problems: doc
            .problems
            .iter()
            .map(|p| ProblemDto {
                message: p.message.clone(),
                at: p.at,
            })
            .collect(),
    }
}

fn summary_dto(s: &Summary) -> SummaryDto {
    SummaryDto {
        pages: s.pages,
        fields: s.fields,
        resources: s.resources,
        byte_size: s.byte_size,
        problems: s.problems,
    }
}

fn resource_dto(r: &Resource) -> ResourceDto {
    ResourceDto {
        name: r.name.clone(),
        kind: resource_kind_label(r.kind),
        node_index: r.node_index,
        start: r.record_range.start,
        end: r.record_range.end,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageDto {
    /// `"jpeg"` or `"unsupported"`.
    pub format: &'static str,
    /// Base64 image bytes (empty when unsupported).
    pub base64: String,
}

pub fn image_dto(img: &ExtractedImage, base64: String) -> ImageDto {
    ImageDto {
        format: match img.format {
            ImageFormat::Jpeg => "jpeg",
            ImageFormat::Unsupported => "unsupported",
        },
        base64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_simple_document() {
        let bytes = afpgen::simple();
        let doc = Document::parse(&bytes).unwrap();
        let dto = to_document_dto(&doc, &bytes, "doc1".into(), "simple.afp".into());
        assert_eq!(dto.doc_id, "doc1");
        assert_eq!(dto.file_name, "simple.afp");
        assert_eq!(dto.summary.pages, 1);
        assert!(dto.problems.is_empty());
        // Root is null-indexed; first real child is the Begin Document.
        assert_eq!(dto.root.index, None);
        let bdt = &dto.root.children[0];
        assert_eq!(bdt.sfid, "D3A8A8");
        assert_eq!(bdt.name, "Begin Document (BDT)");
        assert!(bdt.known);
        assert_eq!(bdt.kind, "Begin");
    }

    #[test]
    fn unknown_sfid_is_labeled_with_hex() {
        // A lone self-contained field with an unknown SFID.
        let bytes = afp_core::build::StreamBuilder::new()
            .other([0x12, 0x34, 0x56], &[])
            .build();
        let doc = Document::parse(&bytes).unwrap();
        let dto = to_document_dto(&doc, &bytes, "d".into(), "x.afp".into());
        let node = &dto.root.children[0];
        assert_eq!(node.sfid, "123456");
        assert_eq!(node.name, "Unknown (123456)");
        assert!(!node.known);
    }
}
