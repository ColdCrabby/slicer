//! WebAssembly bindings for the profile system.
//!
//! Lets the Angular UI resolve a profile selection into concrete slicing
//! parameters **in-browser**, using the exact same code the native/server path
//! uses — one implementation of the resolution rules, no parallel TypeScript.

use wasm_bindgen::prelude::*;

use super::export::{export_library, ProfileExportFormat};
use super::library::ProfileLibrary;
use super::resolve::ProfileSelection;

/// Resolve a profile selection + sparse override diff into concrete
/// [`crate::settings::params::SlicingParams`].
///
/// `selection` is a JS object matching [`ProfileSelection`]:
/// `{ printer, filament, process, overrides }`.
#[wasm_bindgen(js_name = resolveSliceParams)]
pub fn resolve_slice_params(selection: JsValue) -> Result<JsValue, JsValue> {
    let selection: ProfileSelection = serde_wasm_bindgen::from_value(selection)
        .map_err(|e| JsValue::from_str(&format!("invalid profile selection: {e}")))?;
    let params = selection
        .resolve()
        .map_err(|e| JsValue::from_str(&format!("failed to resolve profiles: {e}")))?;
    serde_wasm_bindgen::to_value(&params).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Export a profile library as downloadable TOML — the same renderer the
/// native and server builds use.
///
/// In the web runtime the browser *is* the engine: there is no `profiles.toml`
/// on a server to read, so the caller passes its own library
/// (`{ printers, filaments, processes, labels }`) in.
///
/// `format` is `"bundle"` (a ZIP with one TOML per profile) or `"toml"` (a
/// single `profiles.toml`). Returns
/// `{ filename, mime, bytes }` with `bytes` as a `Uint8Array`.
#[wasm_bindgen(js_name = exportProfileLibrary)]
pub fn export_profile_library(library: JsValue, format: &str) -> Result<JsValue, JsValue> {
    let library: ProfileLibrary = serde_wasm_bindgen::from_value(library)
        .map_err(|e| JsValue::from_str(&format!("invalid profile library: {e}")))?;
    let format = ProfileExportFormat::parse(format)
        .ok_or_else(|| JsValue::from_str(&format!("unknown export format '{format}'")))?;

    let artifact = export_library(&library, format)
        .map_err(|e| JsValue::from_str(&format!("failed to export profiles: {e}")))?;

    let result = js_sys::Object::new();
    js_sys::Reflect::set(
        &result,
        &JsValue::from_str("filename"),
        &JsValue::from_str(&artifact.filename),
    )?;
    js_sys::Reflect::set(
        &result,
        &JsValue::from_str("mime"),
        &JsValue::from_str(artifact.mime),
    )?;
    js_sys::Reflect::set(
        &result,
        &JsValue::from_str("bytes"),
        &js_sys::Uint8Array::from(artifact.bytes.as_slice()),
    )?;
    Ok(result.into())
}
