//! WebAssembly bindings for the profile system.
//!
//! Lets the Angular UI resolve a profile selection into concrete slicing
//! parameters **in-browser**, using the exact same code the native/server path
//! uses — one implementation of the resolution rules, no parallel TypeScript.

use wasm_bindgen::prelude::*;

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
