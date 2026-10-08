//! Assemble a renderable layout for a page: its geometry (from the Page
//! Descriptor) and its positioned text (from PTOCA).
//!
//! Best-effort and resilient: a missing or malformed Page Descriptor falls back
//! to US Letter at 1440 L-units/inch so a page always has sane dimensions.

use crate::names::{self, Kind};
use crate::ptoca;
use crate::tree::{Document, Node};

/// A text run placed in page coordinates (L-units, top-left origin).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PositionedText {
    pub x: i32,
    pub y: i32,
    pub text: String,
}

/// A page ready to render.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PageLayout {
    pub width_lu: i32,
    pub height_lu: i32,
    pub units_per_inch: f32,
    pub texts: Vec<PositionedText>,
}

const PAGE_CAT: u8 = 0xAF; // Begin Page category
const PGD: [u8; 3] = [0xD3, 0xA6, 0xAF]; // Page Descriptor
const PTX: [u8; 3] = [0xD3, 0xEE, 0x9B]; // Presentation Text

// US Letter at 1440 L-units/inch — the fallback geometry.
const DEFAULT_WIDTH_LU: i32 = 12240;
const DEFAULT_HEIGHT_LU: i32 = 15840;
const DEFAULT_UPI: f32 = 1440.0;

impl Document {
    /// Number of pages (Begin Page fields).
    pub fn page_count(&self) -> usize {
        let mut n = 0;
        count_pages(&self.root, &mut n);
        n
    }

    /// Build the renderable layout for the `page_index`-th page (0-based).
    pub fn page_layout(&self, page_index: usize, buf: &[u8]) -> Option<PageLayout> {
        let mut pages = Vec::new();
        collect_pages(&self.root, &mut pages);
        let page = pages.get(page_index)?;

        let (width_lu, height_lu, units_per_inch) = geometry(page, buf);

        let mut texts = Vec::new();
        collect_text(page, buf, &mut texts);

        Some(PageLayout {
            width_lu,
            height_lu,
            units_per_inch,
            texts,
        })
    }
}

fn count_pages(node: &Node, n: &mut usize) {
    for child in &node.children {
        if is_page(child) {
            *n += 1;
        }
        count_pages(child, n);
    }
}

fn collect_pages<'a>(node: &'a Node, out: &mut Vec<&'a Node>) {
    for child in &node.children {
        if is_page(child) {
            out.push(child);
        }
        collect_pages(child, out);
    }
}

fn is_page(node: &Node) -> bool {
    names::classify(node.sfid) == Kind::Begin && names::category(node.sfid) == PAGE_CAT
}

/// Read geometry from the page's Page Descriptor, or fall back to US Letter.
fn geometry(page: &Node, buf: &[u8]) -> (i32, i32, f32) {
    if let Some(pgd) = find_first(page, PGD) {
        let d = &buf[pgd.data_range.start.min(buf.len())..pgd.data_range.end.min(buf.len())];
        if d.len() >= 12 {
            let x_units = u16::from_be_bytes([d[2], d[3]]) as f32;
            // XpUnitBase 0x00 == 10 inches, so units-per-inch = XpUnits / 10.
            let upi = if d[0] == 0x00 && x_units > 0.0 {
                x_units / 10.0
            } else {
                DEFAULT_UPI
            };
            let width = be_u24(&d[6..9]);
            let height = be_u24(&d[9..12]);
            if width > 0 && height > 0 {
                return (width, height, upi);
            }
        }
    }
    (DEFAULT_WIDTH_LU, DEFAULT_HEIGHT_LU, DEFAULT_UPI)
}

fn collect_text(node: &Node, buf: &[u8], out: &mut Vec<PositionedText>) {
    for child in &node.children {
        if child.sfid == PTX {
            let d =
                &buf[child.data_range.start.min(buf.len())..child.data_range.end.min(buf.len())];
            for run in ptoca::parse_text(d) {
                out.push(PositionedText {
                    x: run.x,
                    y: run.y,
                    text: run.text,
                });
            }
        }
        collect_text(child, buf, out);
    }
}

/// First descendant (or self's children) whose SFID matches.
fn find_first(node: &Node, sfid: [u8; 3]) -> Option<&Node> {
    for child in &node.children {
        if child.sfid == sfid {
            return Some(child);
        }
        if let Some(found) = find_first(child, sfid) {
            return Some(found);
        }
    }
    None
}

fn be_u24(b: &[u8]) -> i32 {
    ((b[0] as i32) << 16) | ((b[1] as i32) << 8) | (b[2] as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_pages() {
        let bytes = afpgen::multi_page();
        let doc = Document::parse(&bytes).unwrap();
        assert_eq!(doc.page_count(), 3);
    }

    #[test]
    fn first_page_has_geometry_and_text() {
        let bytes = afpgen::simple();
        let doc = Document::parse(&bytes).unwrap();
        let layout = doc.page_layout(0, &bytes).unwrap();
        assert_eq!(layout.width_lu, 12240);
        assert_eq!(layout.height_lu, 15840);
        assert_eq!(layout.units_per_inch, 1440.0);
        assert_eq!(layout.texts.len(), 2);
        assert_eq!(layout.texts[0], PositionedText { x: 1440, y: 1440, text: "HELLO AFP".into() });
    }

    #[test]
    fn out_of_range_page_is_none() {
        let bytes = afpgen::simple();
        let doc = Document::parse(&bytes).unwrap();
        assert!(doc.page_layout(9, &bytes).is_none());
    }

    #[test]
    fn page_without_pgd_uses_default_geometry() {
        use crate::build::StreamBuilder;
        let bpg = [0xD3, 0xA8, 0xAF];
        let epg = [0xD3, 0xA9, 0xAF];
        let bytes = StreamBuilder::new().begin(bpg).end(epg).build();
        let doc = Document::parse(&bytes).unwrap();
        let layout = doc.page_layout(0, &bytes).unwrap();
        assert_eq!(layout.width_lu, DEFAULT_WIDTH_LU);
        assert_eq!(layout.units_per_inch, DEFAULT_UPI);
        assert!(layout.texts.is_empty());
    }
}
