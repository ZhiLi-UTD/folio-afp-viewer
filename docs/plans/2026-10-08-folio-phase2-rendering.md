# Folio Phase 2 (Page Rendering) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Render AFP pages visually — positioned text laid out on a correctly-sized page canvas — with page navigation, zoom/fit, and export-to-PNG, as a third "Render" view alongside the Phase 1 inspector.

**Architecture:** Two new pure-Rust modules in `afp-core`: `ptoca` decodes PTOCA presentation-text control sequences (moves + transparent data) into absolutely-positioned text runs; `page` reads the Page Descriptor (PGD) geometry and assembles, per page, a `PageLayout { width_lu, height_lu, units_per_inch, texts, images }`. A new IPC command returns a page layout; the front-end draws it on an HTML `<canvas>` scaled from L-units to pixels. Font fidelity is best-effort (a substitute sans sized from the text's inline advance); image placement is best-effort and may land in a later pass.

**Tech stack:** Same as Phase 1. No new dependencies expected (canvas is native; JPEG images reuse the Phase 1 extraction + data URLs).

**Scope guard (YAGNI):** MVP renders positioned TEXT + page outline + nav/zoom/export. Precise image placement, GOCA graphics, bar codes, overlays, and true FOCA font metrics are explicitly out of this plan.

**Key facts used:**
- A Page Descriptor (PGD) carries measurement units (L-units per 10 inches or per base) and the page X/Y extent. We derive `units_per_inch` and page size in L-units from it.
- Presentation Text (PTX) data is a chain of PTOCA control sequences. The data begins with the introducer `0x2B 0xD3`; thereafter each control sequence is `[length][type][parameters]`, where `length` counts itself through the parameters. Relevant types: Absolute Move Inline **AMI 0xC7**, Absolute Move Baseline **AMB 0xD3**, Relative Move Inline **RMI 0xC9**, Relative Move Baseline **RMB 0xD5**, Transparent Data **TRN 0xDB** (the characters, EBCDIC), Set Coded Font Local **SCFL 0xF1**, Set Text Orientation **STO 0xF7**. (Low bit of the type byte is the chaining flag; mask it off when matching.)

---

## Task 1: PTOCA control-sequence decoder → positioned text runs

**Files:** Create `crates/afp-core/src/ptoca.rs`; add `pub mod ptoca;`.

**Model:**
```rust
pub struct TextRun { pub x: i32, pub y: i32, pub text: String } // x,y in L-units, top-left origin
pub fn parse_text(data: &[u8]) -> Vec<TextRun>
```

**Behaviour:** skip a leading `0x2B 0xD3`; then loop `[length][type][params]` (resilient — stop on bad length). Track `cur_x`, `cur_y` (start 0,0). AMI sets x (2-byte big-endian), AMB sets y, RMI/RMB add to x/y. TRN decodes params as EBCDIC (reuse `resource::decode_ebcdic`) and emits a `TextRun { x: cur_x, y: cur_y, text }`, then advances cur_x by an estimated width (params length × a nominal char width) so consecutive TRNs without a move don't overlap. Unknown types are skipped by their length.

**Tests (TDD):** build a PTX data buffer with AMI/AMB/TRN via a small test helper; assert one run at the expected (x,y) with decoded text; assert resilient stop on truncated control sequence; assert RMI adds to x.

---

## Task 2: afpgen emits real PTOCA text

**Files:** Modify `tools/afpgen/src/lib.rs`.

Replace the placeholder PTX payloads with a `ptoca_line(x, y, text)` helper that emits `2B D3` + AMB(y) + AMI(x) + TRN(ebcdic(text)). Update `simple`, `multi_page`, `with_image` to place a couple of lines at real coordinates. Keep page counts unchanged (existing roundtrip tests must still pass).

**Test:** an afp-core integration test parses `simple()` and asserts the first page yields a text run with the expected string and coordinates.

---

## Task 3: Page geometry from PGD + PageLayout assembly

**Files:** Create `crates/afp-core/src/page.rs`; add `pub mod page;`.

**Model:**
```rust
pub struct PositionedText { pub x: i32, pub y: i32, pub text: String }
pub struct PageLayout {
    pub width_lu: i32, pub height_lu: i32, pub units_per_inch: f32,
    pub texts: Vec<PositionedText>,
}
impl Document {
    pub fn page_count(&self) -> usize;                 // Begin Page nodes
    pub fn page_layout(&self, page_index: usize, buf: &[u8]) -> Option<PageLayout>;
}
```

**Behaviour:** find the Nth Begin Page node; parse its child PGD (if present) for units + extent (fallback to US Letter at 1440/inch when absent or unparseable); collect its descendant PTX fields, run `ptoca::parse_text` on each, offsetting into page coordinates; return the layout. Resilient: missing/garbled PGD → sensible default page.

**Tests:** `page_count` on `multi_page()` == 3; `page_layout(0)` on `simple()` returns the expected text run and a positive page size.

---

## Task 4: IPC command `get_page_layout`

**Files:** Modify `src-tauri/src/lib.rs`, add a `PageLayoutDto` to `src-tauri/src/dto.rs`.

`get_page_layout(doc_id, page_index) -> PageLayoutDto { widthLu, heightLu, unitsPerInch, pageCount, texts: [{x,y,text}] }`. Re-parse stored bytes; map. Unit-test the DTO mapping (pure) like Phase 1.

---

## Task 5: Render view (canvas) in the front-end

**Files:** Modify `index.html`, `src/styles.css`, `src/main.ts`, `src/api.ts`.

- Add a third segment: **Structure · Resources · Render**.
- Render view: a page canvas centered on a neutral "desk" surface; a slim page-nav bar (‹ Page n / N ›), a zoom control (Fit / 100% / +/−), and an **Export PNG** button.
- Draw: white page rect scaled `px = lu / units_per_inch * (96 * zoom)`; draw each text run at its scaled position with a substitute font; subtle page shadow. Fit computes zoom so the page fits the viewport.
- Export: `canvas.toDataURL('image/png')` → download via an anchor.
- Keyboard: ←/→ change pages when the Render view is active.

**Verify:** open a generated fixture in Chrome preview or the dev app; confirm pages render with text in plausible positions, nav + zoom + export work, light and dark both look right. Screenshot.

---

## Task 6: Polish + guardrails
- Empty/edge: a page with no text shows an empty page outline, not a blank canvas.
- Large pages: cap canvas backing size and scale down (avoid giant allocations).
- Note in README that rendering is best-effort text layout (Phase 2 MVP).

---

## Done criteria (Phase 2 MVP)
- `cargo test --workspace` green (new ptoca/page/dto tests included).
- Render view shows positioned text on correctly-sized pages for the fixtures; nav, zoom/fit, export PNG work; light + dark verified.
- Parser still never panics on `malformed.afp`.
- No new dependencies (or any addition called out).
