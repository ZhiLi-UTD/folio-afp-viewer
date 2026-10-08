//! Folio Tauri backend: opens AFP files and serves decoded views to the webview.

mod dto;

use afp_core::Document;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use dto::{
    image_dto, page_layout_dto, to_document_dto, DocumentDto, ImageDto, PageLayoutDto,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::State;

/// Opened files, keyed by the id returned from `open_afp`.
#[derive(Default)]
struct Store {
    files: Mutex<HashMap<String, Vec<u8>>>,
    counter: AtomicU64,
}

fn file_name_of(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// Read and parse an AFP file; returns the decoded document view-model.
#[tauri::command]
fn open_afp(path: String, store: State<Store>) -> Result<DocumentDto, String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("Could not read file: {e}"))?;
    let doc = Document::parse(&bytes).map_err(|e| match e {
        afp_core::sf::ParseError::NotAfp => {
            "This file does not look like an AFP (MO:DCA) data stream.".to_string()
        }
        afp_core::sf::ParseError::Truncated { at } => {
            format!("The AFP stream is truncated near byte offset {at}.")
        }
    })?;

    let id = store.counter.fetch_add(1, Ordering::Relaxed);
    let doc_id = format!("doc{id}");
    let dto = to_document_dto(&doc, &bytes, doc_id.clone(), file_name_of(&path));
    let mut files = store.files.lock().expect("store lock");
    // The front-end views one document at a time; drop any previously opened
    // buffers so repeatedly opening files does not grow memory without bound.
    files.clear();
    files.insert(doc_id, bytes);
    Ok(dto)
}

/// Return a slice of the raw file bytes, base64-encoded, for the hex view.
#[tauri::command]
fn get_hex_slice(
    doc_id: String,
    start: usize,
    len: usize,
    store: State<Store>,
) -> Result<String, String> {
    let files = store.files.lock().expect("store lock");
    let bytes = files.get(&doc_id).ok_or("Unknown document id")?;
    let end = start.saturating_add(len).min(bytes.len());
    let start = start.min(bytes.len());
    Ok(STANDARD.encode(&bytes[start..end]))
}

/// Extract a previewable image for the resource/object at `node_index`.
#[tauri::command]
fn get_resource_bytes(
    doc_id: String,
    node_index: usize,
    store: State<Store>,
) -> Result<ImageDto, String> {
    let files = store.files.lock().expect("store lock");
    let bytes = files.get(&doc_id).ok_or("Unknown document id")?;
    let doc = Document::parse(bytes).map_err(|_| "Document could not be re-parsed")?;
    let img = doc
        .extract_image(node_index, bytes)
        .ok_or("No such node in document")?;
    let encoded = STANDARD.encode(&img.bytes);
    Ok(image_dto(&img, encoded))
}

/// Build the renderable layout for one page.
#[tauri::command]
fn get_page_layout(
    doc_id: String,
    page_index: usize,
    store: State<Store>,
) -> Result<PageLayoutDto, String> {
    let files = store.files.lock().expect("store lock");
    let bytes = files.get(&doc_id).ok_or("Unknown document id")?;
    let doc = Document::parse(bytes).map_err(|_| "Document could not be re-parsed")?;
    let count = doc.page_count();
    let layout = doc
        .page_layout(page_index, bytes)
        .ok_or("No such page in document")?;
    Ok(page_layout_dto(&layout, count))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Store::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            open_afp,
            get_hex_slice,
            get_resource_bytes,
            get_page_layout
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
