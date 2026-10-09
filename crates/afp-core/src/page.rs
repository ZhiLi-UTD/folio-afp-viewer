//! Assemble a renderable layout for a page: its geometry (from the Page
//! Descriptor) and its positioned text (from PTOCA).
//!
//! Best-effort and resilient: a missing or malformed Page Descriptor falls back
//! to US Letter at 1440 L-units/inch so a page always has sane dimensions.

use crate::names::{self, Kind};
use crate::tree::{Document, Node};
use crate::{font, ptoca};

/// A text run placed in page coordinates (L-units, top-left origin).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PositionedText {
    pub x: i32,
    pub y: i32,
    pub text: String,
    /// Text height in L-units: the selected font's point size (FOCA/MCF) when
    /// known, otherwise the page's line-spacing estimate.
    pub font_size_lu: i32,
}

/// An image placed on the page, in page coordinates (L-units). `node_index`
/// resolves to the image object so the front-end can fetch its bytes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PositionedImage {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub node_index: usize,
}

/// A page ready to render.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PageLayout {
    pub width_lu: i32,
    pub height_lu: i32,
    pub units_per_inch: f32,
    /// Estimated text height in L-units (from line spacing; no FOCA metrics yet).
    pub font_size_lu: i32,
    pub texts: Vec<PositionedText>,
    pub images: Vec<PositionedImage>,
}

const PAGE_CAT: u8 = 0xAF; // Begin Page category
const IMAGE_CAT: u8 = 0xFB; // Begin Image Object category
const PGD: [u8; 3] = [0xD3, 0xA6, 0xAF]; // Page Descriptor
const PTX: [u8; 3] = [0xD3, 0xEE, 0x9B]; // Presentation Text
const BPT: [u8; 3] = [0xD3, 0xA8, 0x9B]; // Begin Presentation Text (object)
const MCF: [u8; 3] = [0xD3, 0xAB, 0x8A]; // Map Coded Font (format 2)
const FONT_UNITS_PER_INCH: f32 = 1440.0; // MCF font sizes are at 1440 upi
const OBP: [u8; 3] = [0xD3, 0xAC, 0x6B]; // Object Area Position
const OBD: [u8; 3] = [0xD3, 0xA6, 0x6B]; // Object Area Descriptor

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

        // Font point sizes per local id, merged across all Map Coded Font
        // records in the page (a page's font mappings may span several MCFs).
        let mut font_map = std::collections::HashMap::new();
        collect_font_map(page, buf, &mut font_map);

        // Gather raw PTOCA runs (each tagged with its selected font id),
        // threading the cursor across PTX records.
        let mut runs = Vec::new();
        let mut cur = ptoca::Cursor::default();
        collect_runs(page, buf, &mut cur, &mut runs);

        // Line-spacing estimate is the fallback for runs with no mapped font.
        let fallback = estimate_font_size(&runs, units_per_inch);
        let texts = runs
            .iter()
            .map(|r| PositionedText {
                x: r.x,
                y: r.y,
                text: r.text.clone(),
                // MCF sizes are at 1440 upi; convert to the page's units.
                font_size_lu: r
                    .font_id
                    .and_then(|id| font_map.get(&id).copied())
                    .map(|sz| (sz as f32 * units_per_inch / FONT_UNITS_PER_INCH) as i32)
                    .unwrap_or(fallback),
            })
            .collect();

        let mut images = Vec::new();
        collect_images(page, buf, &mut images);

        Some(PageLayout {
            width_lu,
            height_lu,
            units_per_inch,
            font_size_lu: fallback,
            texts,
            images,
        })
    }
}

/// Estimate text height from the smallest positive gap between successive text
/// baselines (line pitch ≈ 1.2 × text height). Falls back to 10pt-equivalent.
fn estimate_font_size(runs: &[ptoca::TextRun], upi: f32) -> i32 {
    let mut ys: Vec<i32> = runs.iter().map(|t| t.y).collect();
    ys.sort_unstable();
    ys.dedup();
    let min_gap = ys.windows(2).map(|w| w[1] - w[0]).filter(|&g| g > 0).min();
    match min_gap {
        Some(gap) => {
            let size = (gap as f32 / 1.2) as i32;
            // Clamp to a sane 6pt–18pt range in L-units.
            let lo = (6.0 / 72.0 * upi) as i32;
            let hi = (18.0 / 72.0 * upi) as i32;
            size.clamp(lo, hi)
        }
        None => (10.0 / 72.0 * upi) as i32,
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

fn is_image(node: &Node) -> bool {
    names::classify(node.sfid) == Kind::Begin && names::category(node.sfid) == IMAGE_CAT
}

/// Collect image objects placed directly on the page, with position/size taken
/// from their Object Area Position (OBP) and Object Area Descriptor (OBD).
///
/// This parses a pragmatic subset of OBP/OBD sufficient to place an image;
/// full object-area coordinate systems are future work. An image with no
/// position defaults to the page origin; no size defaults to a quarter page.
fn collect_images(node: &Node, buf: &[u8], out: &mut Vec<PositionedImage>) {
    for child in &node.children {
        if is_image(child) {
            let (x, y) = image_position(child, buf);
            let (w, h) = image_size(child, buf);
            out.push(PositionedImage {
                x,
                y,
                w,
                h,
                node_index: child.index,
            });
        } else {
            // Images nest inside page/resource groups, not inside each other.
            collect_images(child, buf, out);
        }
    }
}

/// X/Y origin from the OBP (Object Area Position): `[id][X(3)][Y(3)]`.
fn image_position(img: &Node, buf: &[u8]) -> (i32, i32) {
    if let Some(obp) = find_first(img, OBP) {
        let d = slice(buf, obp);
        if d.len() >= 7 {
            return (be_u24(&d[1..4]), be_u24(&d[4..7]));
        }
    }
    (0, 0)
}

/// Width/height from the OBD (Object Area Descriptor), PGD-like:
/// `[xbase][ybase][xunits(2)][yunits(2)][xsize(3)][ysize(3)]`.
fn image_size(img: &Node, buf: &[u8]) -> (i32, i32) {
    if let Some(obd) = find_first(img, OBD) {
        let d = slice(buf, obd);
        if d.len() >= 12 {
            let w = be_u24(&d[6..9]);
            let h = be_u24(&d[9..12]);
            if w > 0 && h > 0 {
                return (w, h);
            }
        }
    }
    (DEFAULT_WIDTH_LU / 2, DEFAULT_HEIGHT_LU / 4)
}

fn slice<'a>(buf: &'a [u8], node: &Node) -> &'a [u8] {
    &buf[node.data_range.start.min(buf.len())..node.data_range.end.min(buf.len())]
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

/// Merge the font maps of every Map Coded Font record found under the page.
fn collect_font_map(node: &Node, buf: &[u8], out: &mut std::collections::HashMap<u8, i32>) {
    for child in &node.children {
        if child.sfid == MCF {
            for (id, size) in font::font_size_map(slice(buf, child)) {
                out.insert(id, size);
            }
        }
        collect_font_map(child, buf, out);
    }
}

/// Collect raw PTOCA text runs, threading the decoding cursor (position +
/// active font) across the PTX records of one presentation-text object. Each
/// Begin Presentation Text object starts with a fresh cursor, since position
/// and font selection do not carry across independent text objects.
fn collect_runs(node: &Node, buf: &[u8], cur: &mut ptoca::Cursor, out: &mut Vec<ptoca::TextRun>) {
    for child in &node.children {
        if child.sfid == PTX {
            let d =
                &buf[child.data_range.start.min(buf.len())..child.data_range.end.min(buf.len())];
            ptoca::parse_into(d, cur, out);
        }
        if child.sfid == BPT {
            // New text object: independent position and font selection.
            let mut inner = ptoca::Cursor::default();
            collect_runs(child, buf, &mut inner, out);
        } else {
            collect_runs(child, buf, cur, out);
        }
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
        let t0 = &layout.texts[0];
        assert_eq!((t0.x, t0.y, t0.text.as_str()), (1440, 1440, "HELLO AFP"));
    }

    #[test]
    fn font_size_comes_from_map_coded_font() {
        let bytes = afpgen::with_fonts();
        let doc = Document::parse(&bytes).unwrap();
        let layout = doc.page_layout(0, &bytes).unwrap();
        // Line 1 selects font 2 (14pt = 280 lu); line 2 selects font 1 (10pt = 200).
        assert_eq!(layout.texts[0].font_size_lu, 280);
        assert_eq!(layout.texts[1].font_size_lu, 200);
    }

    #[test]
    fn page_image_is_positioned_and_sized() {
        let bytes = afpgen::with_page_image();
        let doc = Document::parse(&bytes).unwrap();
        let layout = doc.page_layout(0, &bytes).unwrap();
        assert_eq!(layout.images.len(), 1);
        let img = &layout.images[0];
        assert_eq!((img.x, img.y), (1440, 2880));
        assert_eq!((img.w, img.h), (4320, 2880));
    }

    #[test]
    fn font_size_tracks_line_spacing() {
        // simple() places lines 720 L-units apart -> ~600 L-units text height,
        // clamped to the 18pt max (450 at 1440 upi).
        let bytes = afpgen::simple();
        let doc = Document::parse(&bytes).unwrap();
        let layout = doc.page_layout(0, &bytes).unwrap();
        let max_18pt = (18.0 / 72.0 * 1440.0) as i32;
        assert_eq!(layout.font_size_lu, max_18pt);
    }

    #[test]
    fn active_font_does_not_leak_across_text_objects() {
        let bytes = afpgen::two_text_objects();
        let doc = Document::parse(&bytes).unwrap();
        let layout = doc.page_layout(0, &bytes).unwrap();
        // First object selected font 1 (200 L-units); second selected none, so
        // it must fall back rather than inherit font 1's size.
        assert_eq!(layout.texts[0].font_size_lu, 200);
        assert_ne!(layout.texts[1].font_size_lu, 200);
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
