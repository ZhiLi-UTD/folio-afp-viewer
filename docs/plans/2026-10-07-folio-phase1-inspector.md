# Folio Phase 1 (AFP Structure Inspector) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Ship a portable Windows app with an Apple-style UI that opens `.afp` files and lets a user explore the MO:DCA structured-field tree, inspect each field with a synchronized hex view, browse embedded resources (with image previews), and search/summarize the document.

**Architecture:** A pure Rust library crate `afp-core` parses the AFP byte stream into a `Document` model (a tree of structured-field nodes + a flat index + extracted resources). A Tauri v2 shell exposes `afp-core` to a TypeScript/Vite front-end over IPC, streaming only decoded metadata to the webview and fetching raw hex/resource bytes on demand. The front-end is a hand-built CSS design system (no heavy UI framework) delivering a Finder/Xcode three-pane experience.

**Tech Stack:** Rust (std-first parsing), Tauri v2, TypeScript, Vite, Lucide icons. Tests: Rust `cargo test` (unit + snapshot via `insta` only if justified; start with plain asserts). CI: GitHub Actions Windows runner → portable `.exe`.

**Key correctness insight used throughout:** An AFP structured field is introduced by `0x5A`, followed by a 2-byte big-endian length (counting from the length bytes to the end of the field data, i.e. the full record length minus the 1-byte `0x5A`), a 3-byte SFID, a 1-byte flag, 2 reserved bytes, then the field data. In the 3-byte SFID the **second byte encodes function**: `0xA8` = *Begin*, `0xA9` = *End*; the **third byte encodes the object category**, and a Begin/End pair shares the same third byte. This lets us build a correct nesting tree even before the human-readable name table is complete.

---

## Task 0: Toolchain check + project scaffold

**Files:**
- Create: `crates/afp-core/Cargo.toml`, `crates/afp-core/src/lib.rs`
- Create: Tauri app under `src-tauri/` + front-end under `src/` (via scaffolder)
- Modify: root `Cargo.toml` (workspace)

**Step 1: Verify toolchain**

Run:
```bash
rustc --version && cargo --version && node --version && npm --version
```
Expected: all four print versions. If Rust missing: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`. If Node missing: install Node LTS (brew: `brew install node`).

**Step 2: Scaffold the Tauri + Vite + TS app**

Run from `~/folio`:
```bash
npm create tauri-app@latest . -- --template vanilla-ts --manager npm --identifier app.folio.viewer --yes
```
Expected: creates `src/`, `src-tauri/`, `package.json`, `index.html`. If the dir-not-empty prompt blocks it, scaffold in `tmp-scaffold/` then move files in (keep existing `docs/`, `.git`, `.gitignore`).

**Step 3: Add the `afp-core` workspace crate**

Create `crates/afp-core/Cargo.toml`:
```toml
[package]
name = "afp-core"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
```
Create `crates/afp-core/src/lib.rs`:
```rust
//! AFP (MO:DCA) parsing core — pure, no UI.
pub mod sf;
```
Create root `Cargo.toml` making a workspace that includes `src-tauri` and `crates/afp-core`, and add `afp-core = { path = "../crates/afp-core" }` to `src-tauri/Cargo.toml` dependencies. (serde is already required by Tauri, so no new top-level dependency beyond `afp-core` — call this out: serde is the only added crate in `afp-core`, justified because the model is serialized to the webview.)

**Step 4: Verify it builds and runs**

Run:
```bash
cargo build -p afp-core && npm install && npm run tauri dev
```
Expected: `afp-core` compiles; the Tauri window opens on macOS (dev happens on Mac). Close the window.

**Step 5: Commit**
```bash
git add -A && git commit -m "chore: scaffold Tauri app and afp-core crate"
```

---

## Task 1: Structured-field framing (the parser's foundation)

**Files:**
- Create: `crates/afp-core/src/sf.rs`
- Test: inline `#[cfg(test)]` in `sf.rs`

**Step 1: Write the failing test** — craft raw bytes by hand (no sample files needed).

```rust
// in src/sf.rs
#[cfg(test)]
mod tests {
    use super::*;

    // One structured field: 0x5A, len=0x0010(16), SFID=D3A8A8 (Begin Document),
    // flag=0x00, reserved=0x0000, then 7 bytes of data.
    fn one_bdt() -> Vec<u8> {
        vec![0x5A, 0x00, 0x10, 0xD3, 0xA8, 0xA8, 0x00, 0x00, 0x00,
             1, 2, 3, 4, 5, 6, 7]
    }

    #[test]
    fn parses_single_field() {
        let fields = parse_fields(&one_bdt()).unwrap();
        assert_eq!(fields.len(), 1);
        let f = &fields[0];
        assert_eq!(f.sfid, [0xD3, 0xA8, 0xA8]);
        assert_eq!(f.flag, 0x00);
        assert_eq!(f.data_range, 9..16);     // offsets within the record stream
        assert_eq!(f.record_range, 0..16);
    }

    #[test]
    fn parses_two_fields() {
        let mut bytes = one_bdt();
        bytes.extend(one_bdt()); // second field starts at offset 16
        let fields = parse_fields(&bytes).unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[1].record_range, 16..32);
    }

    #[test]
    fn rejects_non_afp() {
        let err = parse_fields(b"%PDF-1.7").unwrap_err();
        assert!(matches!(err, ParseError::NotAfp));
    }

    #[test]
    fn truncated_field_is_reported_not_panicked() {
        let bytes = vec![0x5A, 0x00, 0x10, 0xD3, 0xA8, 0xA8]; // claims 16, has 6
        let err = parse_fields(&bytes).unwrap_err();
        assert!(matches!(err, ParseError::Truncated { .. }));
    }
}
```

**Step 2: Run to verify it fails**

Run: `cargo test -p afp-core sf::tests`
Expected: FAIL — `parse_fields`/`StructuredField`/`ParseError` not defined.

**Step 3: Minimal implementation**

```rust
use std::ops::Range;

#[derive(Debug, Clone, serde::Serialize)]
pub struct StructuredField {
    pub sfid: [u8; 3],
    pub flag: u8,
    /// Byte range of the field *data* (after the 6-byte introducer+SFID+flag+reserved), within the stream.
    pub data_range: Range<usize>,
    /// Byte range of the whole record including the 0x5A, within the stream.
    pub record_range: Range<usize>,
}

#[derive(Debug)]
pub enum ParseError {
    NotAfp,
    Truncated { at: usize },
}

pub fn parse_fields(buf: &[u8]) -> Result<Vec<StructuredField>, ParseError> {
    if buf.first() != Some(&0x5A) {
        return Err(ParseError::NotAfp);
    }
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < buf.len() {
        if buf[pos] != 0x5A {
            return Err(ParseError::Truncated { at: pos });
        }
        // length is 2 bytes after the 0x5A; counts from itself to end of field data.
        if pos + 3 > buf.len() {
            return Err(ParseError::Truncated { at: pos });
        }
        let len = u16::from_be_bytes([buf[pos + 1], buf[pos + 2]]) as usize;
        let record_end = pos + 1 + len; // +1 for the 0x5A
        if len < 8 || record_end > buf.len() {
            return Err(ParseError::Truncated { at: pos });
        }
        let sfid = [buf[pos + 3], buf[pos + 4], buf[pos + 5]];
        let flag = buf[pos + 6];
        // bytes 7..8 reserved
        let data_start = pos + 9;
        out.push(StructuredField {
            sfid,
            flag,
            data_range: data_start..record_end,
            record_range: pos..record_end,
        });
        pos = record_end;
    }
    Ok(out)
}
```

**Step 4: Run to verify pass**

Run: `cargo test -p afp-core sf::tests`
Expected: PASS (4 tests).

**Step 5: Commit**
```bash
git add -A && git commit -m "feat(core): structured-field framing with resilient errors"
```

---

## Task 2: SFID classification (Begin/End/category) + name table

**Files:**
- Create: `crates/afp-core/src/names.rs`
- Modify: `crates/afp-core/src/lib.rs` (add `pub mod names;`)

**Step 1: Failing test**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifies_begin_end() {
        assert_eq!(classify([0xD3,0xA8,0xA8]), Kind::Begin);
        assert_eq!(classify([0xD3,0xA9,0xA8]), Kind::End);
        assert_eq!(classify([0xD3,0xEE,0x9B]), Kind::Other); // PTX
    }
    #[test]
    fn begin_end_pair_shares_category() {
        assert_eq!(category([0xD3,0xA8,0xAF]), category([0xD3,0xA9,0xAF]));
    }
    #[test]
    fn known_name_lookup() {
        assert_eq!(name([0xD3,0xA8,0xA8]), Some("Begin Document (BDT)"));
        assert_eq!(name([0x00,0x00,0x00]), None);
    }
}
```

**Step 2:** `cargo test -p afp-core names` → FAIL.

**Step 3: Implement**
```rust
#[derive(Debug, PartialEq, Eq, Clone, Copy, serde::Serialize)]
pub enum Kind { Begin, End, Other }

pub fn classify(sfid: [u8;3]) -> Kind {
    match sfid[1] {
        0xA8 => Kind::Begin,
        0xA9 => Kind::End,
        _ => Kind::Other,
    }
}
pub fn category(sfid: [u8;3]) -> u8 { sfid[2] }

/// Human-readable names. Start with a verified subset; extend from the
/// IBM MO:DCA Reference (SC31-6802) and AFP data-stream docs over time.
/// Unknown SFIDs return None and the UI shows the raw hex.
pub fn name(sfid: [u8;3]) -> Option<&'static str> {
    Some(match sfid {
        [0xD3,0xA8,0xA8] => "Begin Document (BDT)",
        [0xD3,0xA9,0xA8] => "End Document (EDT)",
        [0xD3,0xA8,0xAF] => "Begin Page (BPG)",
        [0xD3,0xA9,0xAF] => "End Page (EPG)",
        [0xD3,0xA8,0xC6] => "Begin Resource Group (BRG)",
        [0xD3,0xA9,0xC6] => "End Resource Group (ERG)",
        [0xD3,0xA8,0xCE] => "Begin Resource (BR)",
        [0xD3,0xA9,0xCE] => "End Resource (ER)",
        [0xD3,0xA8,0x5F] => "Begin Page Segment (BPS)",
        [0xD3,0xA9,0x5F] => "End Page Segment (EPS)",
        [0xD3,0xA8,0xFB] => "Begin Image Object (BIM)",
        [0xD3,0xA9,0xFB] => "End Image Object (EIM)",
        [0xD3,0xEE,0x9B] => "Presentation Text (PTX)",
        [0xD3,0xA8,0x9B] => "Begin Presentation Text (BPT)",
        [0xD3,0xA9,0x9B] => "End Presentation Text (EPT)",
        [0xD3,0xA6,0xAF] => "Page Descriptor (PGD)",
        [0xD3,0xA0,0x90] => "Tag Logical Element (TLE)",
        [0xD3,0xEE,0xEE] => "No Operation (NOP)",
        _ => return None,
    })
}
```
> NOTE to implementer: verify each byte triple against the MO:DCA Reference as you extend the table; keep this list append-only and alphabetized by acronym in comments. Do NOT guess codes — an unknown is safer shown as hex than mislabeled.

**Step 4:** `cargo test -p afp-core names` → PASS. **Step 5:** commit `feat(core): SFID classification + starter name table`.

---

## Task 3: Tree builder (document → pages → objects)

**Files:** Create `crates/afp-core/src/tree.rs`; add `pub mod tree;`.

**Step 1: Failing test** — build a tiny doc: BDT → BPG → PTX → EPG → EDT.
```rust
#[test]
fn builds_nested_tree() {
    let b = TestStream::new()
        .begin([0xD3,0xA8,0xA8])   // BDT
        .begin([0xD3,0xA8,0xAF])   // BPG
        .other([0xD3,0xEE,0x9B], &[0xAB])   // PTX
        .end([0xD3,0xA9,0xAF])     // EPG
        .end([0xD3,0xA9,0xA8])     // EDT
        .build();
    let doc = Document::parse(&b).unwrap();
    assert_eq!(doc.root.children.len(), 1);          // the BDT
    let bdt = &doc.root.children[0];
    assert_eq!(bdt.children.len(), 1);               // the BPG
    assert_eq!(bdt.children[0].children.len(), 1);   // the PTX leaf
}
#[test]
fn mismatched_end_is_recorded_as_problem() {
    let b = TestStream::new().begin([0xD3,0xA8,0xAF]).end([0xD3,0xA9,0xA8]).build();
    let doc = Document::parse(&b).unwrap();
    assert!(!doc.problems.is_empty());
}
```
(Add a `TestStream` helper in the test module that emits valid `0x5A` records; this is the seed of `tools/afpgen`.)

**Step 2:** FAIL. **Step 3:** Implement `Node { sfid, name: Option<String>, kind, index, children, data_range, record_range }`, `Document { root, problems, flat: Vec<usize>, ... }`. Walk `parse_fields`; maintain a stack: `Begin` pushes a node, `End` pops (record a `Problem` if category mismatches), `Other` appends a leaf to the current top. Assign each node an incrementing `index` for the flat search index.

**Step 4:** PASS. **Step 5:** commit `feat(core): nesting tree builder with problem reporting`.

---

## Task 4: Triplet (TLV) decoder + per-field parameters

**Files:** Create `crates/afp-core/src/triplet.rs`; `pub mod triplet;`.

**Step 1: Failing test** — triplets are `[len][id][data...]` where `len` includes itself and the id.
```rust
#[test]
fn decodes_triplet_sequence() {
    // len=4,id=0x02,data=[0xAA,0xBB] ; len=3,id=0x01,data=[0xCC]
    let bytes = [0x04,0x02,0xAA,0xBB, 0x03,0x01,0xCC];
    let t = parse_triplets(&bytes);
    assert_eq!(t.len(), 2);
    assert_eq!(t[0].id, 0x02);
    assert_eq!(t[0].data, vec![0xAA,0xBB]);
    assert_eq!(t[1].id, 0x01);
}
```
**Step 2:** FAIL. **Step 3:** implement resilient `parse_triplets` (stop/record on bad length, never panic) + a `triplet_name(id)->Option<&str>` starter table (e.g. `0x02 Fully Qualified Name`, `0x01 Coded Graphic Character Set Global ID`, `0x24 Resource Local ID`...; extend from the reference, same no-guessing rule). **Step 4:** PASS. **Step 5:** commit `feat(core): generic triplet decoder + starter triplet names`.

---

## Task 5: Document summary + resource enumeration

**Files:** Create `crates/afp-core/src/summary.rs`, `crates/afp-core/src/resource.rs`.

**Step 1: Failing tests**
- `summary`: given a doc with 2 BPG and 1 BIM, `doc.summary().pages == 2`, counts by category, total field count, file size.
- `resource`: walk BRG/BR…ER groups; for each resource capture name (from the FQN triplet on the BR) + category + byte range. A doc with one image resource yields `doc.resources().len() == 1` with `kind == ResourceKind::ImageObject`.

**Step 2:** FAIL. **Step 3:** implement by walking the tree (reuse Task 3 output). Resource kind derived from the Begin category inside the BR wrapper (image object, page segment, overlay, coded font, code page, character set). **Step 4:** PASS. **Step 5:** commit `feat(core): document summary + resource enumeration`.

---

## Task 6: `tools/afpgen` — synthetic AFP file generator

**Files:** Create `tools/afpgen/Cargo.toml`, `tools/afpgen/src/main.rs` (workspace member).

**Why now:** we have no sample files; this produces real `.afp` files to open in the app while building the UI, and feeds integration tests.

**Step 1: Failing test (integration)** in `afp-core`: `tests/roundtrip.rs` — generate a doc with afpgen's library fn, parse it, assert page/resource counts. (Expose afpgen's builders as a `lib.rs` so tests can call them without shelling out.)

**Step 2:** FAIL. **Step 3:** implement builders that emit valid `0x5A` records: `document()`, `.page()`, `.text(str)`, `.image(png_or_jpeg_bytes)`, `.resource_group(...)`, `.build() -> Vec<u8>`. `main.rs` writes a few fixtures to `tools/afpgen/out/*.afp` (e.g. `simple.afp`, `with-image.afp`, `multi-page.afp`, `malformed.afp`). **Step 4:** PASS. **Step 5:** commit `feat(tools): synthetic AFP generator + roundtrip tests`.

Run `cargo run -p afpgen` once and keep the generated fixtures out of git (they're reproducible) — add `tools/afpgen/out/` to `.gitignore`.

---

## Task 7: IOCA image extraction (preview-ready bytes)

**Files:** Create `crates/afp-core/src/ioca.rs`.

**Step 1: Failing test:** given an Image Object produced by afpgen wrapping a JPEG, `extract_image(node, buf)` returns `{ format: Jpeg, bytes }`.
**Step 2:** FAIL.
**Step 3:** parse IOCA self-defining fields to find the Image Data (IDE) and the compression/encoding; for JPEG (FS11) return the embedded JPEG bytes directly (webview can render). For uncompressed/G4 (FS10) return `format: Unsupported` for now (Phase 1 shows "preview not available for this compression"). No image-decoding dependency added in Phase 1 — JPEG is rendered by the webview itself. Call this out.
**Step 4:** PASS. **Step 5:** commit `feat(core): IOCA image extraction (JPEG preview path)`.

---

## Task 8: Tauri IPC commands

**Files:** Modify `src-tauri/src/lib.rs` (or `main.rs`), `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`.

**Commands (each a `#[tauri::command]`):**
- `open_afp(path: String) -> DocumentDto` — read file, `Document::parse`, return the tree + summary + resource list + problems (metadata only; NOT raw bytes). Hold the file bytes in Tauri managed state keyed by a doc id.
- `get_hex_slice(doc_id, start, len) -> Vec<u8>` (base64 or array) — for the hex pane / field bytes, on demand.
- `get_resource_bytes(doc_id, resource_index) -> { format, base64 }` — for image previews.

**Step 1:** write a Rust test for the pure mapping `Document -> DocumentDto` (keep commands thin; logic testable without Tauri). **Step 2:** FAIL. **Step 3:** implement DTOs + mapping + command wrappers; register in the builder; enable the dialog + fs plugins only as needed (call out any plugin added). **Step 4:** `cargo test -p` for the mapping passes; `npm run tauri dev` + manually invoke confirms a generated `.afp` opens. **Step 5:** commit `feat(app): IPC commands open_afp/get_hex_slice/get_resource_bytes`.

---

## Task 9: Front-end design system (the Apple look)

**Files:** Create `src/styles/tokens.css`, `src/styles/base.css`; wire into `index.html`.

Deliver design tokens: system-ui/Inter font stack; light+dark via `prefers-color-scheme`; CSS variables for the blue accent, surfaces, vibrancy (`backdrop-filter: blur(...)`), radii, shadows, spacing scale; spring-ish transition tokens. No framework. Manual verify: a sample toolbar + sidebar row look right in both light and dark (screenshot each). Commit `feat(ui): Apple-style design tokens + base styles`.

---

## Task 10: Three-pane shell + open flow

**Files:** `src/main.ts`, `src/components/*.ts`, `index.html`.

Build: translucent top toolbar (open button + file name + segmented control Structure⇄Resources + search field); left source list pane; center detail pane; bottom hex pane; drag-and-drop `.afp` onto the window → `open_afp`. Render the returned tree in the source list with disclosure triangles (spring animation). Manual verify with generated fixtures (screenshot). Commit `feat(ui): three-pane shell + open/drag-drop flow`.

---

## Task 11: Field detail inspector + synchronized hex

**Files:** `src/components/inspector.ts`, `src/components/hexview.ts`.

Selecting a tree node shows: name (or raw SFID hex if unknown), category, offsets, decoded triplets (Task 4) as grouped property rows; the hex pane fetches `get_hex_slice` for the field's `record_range` and highlights it (offset gutter + hex + ASCII columns). Clicking a byte range in hex highlights the owning field. Manual verify. Commit `feat(ui): field inspector + synchronized hex view`.

---

## Task 12: Resource browser + image preview

**Files:** `src/components/resources.ts`.

Resources segment lists resources grouped by kind with icons; selecting an image resource calls `get_resource_bytes` and previews JPEGs inline (unsupported compressions show a tasteful placeholder + "preview not available"). Fonts/overlays/page-segments show metadata. Manual verify with `with-image.afp`. Commit `feat(ui): resource browser with image previews`.

---

## Task 13: Search + document summary

**Files:** `src/components/search.ts`, `src/components/summary.ts`.

Live-filter the tree by SFID/name/text; a summary panel (page count, per-category counts, field count, file size, detected encoding, problems list). Manual verify. Commit `feat(ui): search + document summary`.

---

## Task 14: Portable Windows build on CI

**Files:** Create `.github/workflows/release.yml`.

Windows runner: `npm ci`, `npm run tauri build`, produce the `.exe`; upload as a release artifact. Configure Tauri bundle for a portable target and document the fixed-WebView2 option. (No local Windows build on the Mac.) Manual verify: push a tag, confirm the artifact appears. Commit `ci: build portable Windows .exe on tagged release`.

---

## Done criteria (Phase 1)
- Open a generated `.afp` on macOS dev build: tree, inspector+hex, resources+image preview, search, summary all work.
- Parser never panics on `malformed.afp`; problems surfaced.
- `cargo test` green; UI screenshots meet the Apple-style bar in light and dark.
- CI produces a portable `.exe`.
- Only added Rust crate is `serde` in `afp-core`; any Tauri plugin additions are listed in the final summary.

**Phase 2 (rendering) is a separate plan — not in scope here.**
