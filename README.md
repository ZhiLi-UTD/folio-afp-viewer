# Folio

An Apple-style AFP viewer for Windows. Folio opens IBM AFP (MO:DCA) files and
lets you explore their structured-field tree, inspect each field with a
synchronized hex view, browse embedded resources (with image previews), and read
a document summary — in a calm, native-macOS-feeling interface.

> Status: **Phase 1 (inspector) + Phase 2 MVP (page rendering)** — see `docs/plans/`.

## What it does today

- **Structure tree** — the full MO:DCA structured-field hierarchy, with decoded
  names (unknown fields shown as raw hex, never mislabeled).
- **Field inspector + hex** — decoded field properties alongside a byte view
  synced to the selection.
- **Resource browser** — images, page segments, overlays and fonts; JPEG image
  resources preview inline.
- **Render** — positioned page text laid out on a correctly-sized page canvas,
  with page navigation, zoom/fit, and export-to-PNG. Rendering is best-effort
  text layout (PTOCA positioning on PGD geometry); precise image placement,
  graphics/bar codes, overlays, and true FOCA font metrics are future work.
- **Search & summary** — live filter plus page/field/resource counts and a
  problems list. The parser is resilient: malformed files never crash it.

## Architecture

| Piece | What it is |
|-------|------------|
| `crates/afp-core` | Pure Rust AFP/MO:DCA parser + document model. No UI. Heavily tested. |
| `src-tauri` | Tauri v2 backend: IPC commands over the parser. |
| `src` | TypeScript + Vite front-end and the Apple-style CSS design system. |
| `tools/afpgen` | Synthetic AFP fixture generator (we build without sample files). |

## Develop (macOS/Linux)

```bash
# one-time: Rust (https://rustup.rs) and Node 20+
npm install
cargo run -p afpgen        # generate sample .afp fixtures in tools/afpgen/out/
npm run tauri dev          # run the app
cargo test --workspace     # run the parser + mapping tests
```

## Windows build

The portable `Folio.exe` is produced by the **Release (Windows portable)** GitHub
Actions workflow on a Windows runner (push a `v*` tag). It relies on the evergreen
WebView2 runtime that ships with Windows 10/11.

## License

TBD.
