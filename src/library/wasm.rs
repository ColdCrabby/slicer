//! WebAssembly bindings for the library.
//!
//! The browser build has no filesystem, so it keeps the [`Library`] document
//! and the bytes in IndexedDB itself — but whether a dropped file is a new
//! object, the same file again, or a re-export of one it already has is decided
//! here, by the same [`Library::import`] every other runtime runs.

use wasm_bindgen::prelude::*;

use super::{Library, LibraryLocation};

/// Record a file into a library document.
///
/// `library` is the current document (or `null` for an empty one); `location`
/// is where the caller will keep the bytes, or `null`. Returns
/// `{ library, outcome }` — the updated document and an
/// [`ImportOutcome`](super::ImportOutcome).
#[wasm_bindgen(js_name = libraryImport)]
pub fn library_import(
    library: JsValue,
    file_name: &str,
    bytes: &[u8],
    location: JsValue,
    now: &str,
) -> Result<JsValue, JsValue> {
    let mut library: Library = if library.is_null() || library.is_undefined() {
        Library::default()
    } else {
        serde_wasm_bindgen::from_value(library)
            .map_err(|e| JsValue::from_str(&format!("invalid library: {e}")))?
    };
    let location: Option<LibraryLocation> = if location.is_null() || location.is_undefined() {
        None
    } else {
        Some(
            serde_wasm_bindgen::from_value(location)
                .map_err(|e| JsValue::from_str(&format!("invalid location: {e}")))?,
        )
    };
    let outcome = library
        .import(file_name, bytes, location, now)
        .map_err(|e| JsValue::from_str(&e))?;

    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    let result = js_sys::Object::new();
    js_sys::Reflect::set(
        &result,
        &JsValue::from_str("library"),
        &serde::Serialize::serialize(&library, &serializer)?,
    )?;
    js_sys::Reflect::set(
        &result,
        &JsValue::from_str("outcome"),
        &serde::Serialize::serialize(&outcome, &serializer)?,
    )?;
    Ok(result.into())
}
