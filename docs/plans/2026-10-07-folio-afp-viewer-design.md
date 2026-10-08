# Design — Folio: Apple-style AFP Viewer for Windows

**Date:** 2026-10-07
**Status:** Approved (brainstorming complete)
**Working name:** Folio (rename candidates: AFP Lens, Vellum)

## One-liner
A friendly, Mac-feeling Windows app that opens `.afp` files, lets you explore their
MO:DCA structure, browse embedded resources, and (Phase 2) render pages.

## Decisions (from brainstorming)
- **Core purpose:** Both, phased — structure *inspector* first (Phase 1), best-effort
  page *rendering* later (Phase 2).
- **Engine:** Build a native MO:DCA parser in Rust from scratch. No commercial SDK
  lock-in. Rendering in Phase 2 starts with cheap wins (images, positioned text,
  rules, overlays); full typographic fidelity is best-effort; optional AFP→PDF path
  kept available if pixel-perfect is ever required.
- **Stack:** Tauri v2 — Rust core (parser + app backend) + web-tech front-end
  (TypeScript + Vite + hand-built CSS design system) for the Apple aesthetic.
- **Samples:** None available — we build a synthetic AFP generator for tests and
  supplement with any public spec sample files.
- **Phase 1 scope:** all four — structured-field tree · field detail + synced hex ·
  resource browser (with image previews) · search + document summary.
- **Distribution:** portable single `.exe`, built on a GitHub Actions Windows runner;
  option to bundle the fixed WebView2 runtime for locked-down machines. Develop/dogfood
  on macOS; release artifacts produced on CI.

## Architecture
```
folio/
├─ crates/afp-core/      Rust: AFP parser + document model (pure, no UI, heavily tested)
├─ src-tauri/            Rust: Tauri shell + IPC commands (open, get hex slice, get resource bytes)
├─ src/                  Front-end: TypeScript + Vite + CSS design system (the Apple look)
├─ tools/afpgen/         Synthetic AFP generator (makes test files; we have no samples)
└─ .github/workflows/    Windows runner builds the portable .exe on release
```
Dependencies kept minimal: `afp-core` uses std-first byte parsing (no heavyweight
parser crate unless justified); front-end is Vite + TS with a tiny icon set (Lucide),
no heavy UI framework. Any new dependency is called out explicitly.

## AFP core (the engine)
- **Framing:** detect format; split the stream into structured fields
  (`0x5A` introducer → 2-byte length → 3-byte SFID → flags → reserved → data; also
  handle record-format variants).
- **SFID decode:** lookup table mapping structured-field IDs → human names
  (BDT/EDT, BPG/EPG, BRG/ERG, PTX, IID, PGD, TLE, etc.).
- **Tree build:** Begin/End pairs (document → pages → objects → resources) form the
  hierarchy; a flat index backs search.
- **Field detail:** generic triplet (TLV) decoder for all fields, plus decoded
  parameters for common field types; unknown fields degrade to labeled raw bytes.
- **Resources:** enumerate fonts (FOCA), overlays, page segments, IOCA images
  (decode + preview where compression allows — JPEG easy; G4/MMR fax best-effort).
- **Resilient:** per-field error capture; never crash on malformed/truncated input;
  surfaces a "Problems" list.

## Data flow
Rust keeps the raw bytes; the webview receives the decoded model (JSON), not the whole
file. Hex slices and resource/image bytes are fetched on demand via IPC. Keeps large
files responsive.

## UI — Apple feel (Phase 1)
Finder/Xcode-style three-pane: translucent toolbar; left source list (segmented
control: Structure ⇄ Resources) with disclosure-triangle tree; center Inspector
(grouped property rows) or resource/image preview; bottom hex inspector (offset gutter
+ bytes + ASCII) synchronized with the selected field.
Design system: system-ui / Inter type, light+dark following the OS, vibrancy/backdrop
blur, rounded corners, soft shadows, spring-eased disclosures/hovers, blue accent,
generous spacing. Drag-and-drop to open; live search field.

## Phase 2 (sketch, not built yet)
Canvas page renderer from parsed positioned objects (PTX text, placed IOCA images,
boxes/rules, overlays); page nav, zoom/fit, export page→PNG. Font fidelity best-effort;
optional AFP→PDF path remains available.

## Error handling
Resilient parser (above) · non-AFP file → friendly message · unknown SFIDs labeled +
raw hex · huge files → streamed with a heads-up.

## Testing
`tools/afpgen` emits valid synthetic AFP streams (document/page/text/image/resources)
→ golden/snapshot tests for framing, triplet decode, tree building, SFID lookup. Plus
any public sample files found. UI verified via screenshots against the Apple-style bar.

## Distribution
GitHub Actions Windows runner → portable single `.exe` (option to bundle fixed WebView2
runtime). Develop on macOS; releases built on CI.
