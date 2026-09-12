//! WebAssembly binding for printer detection.
//!
//! The browser build has no native transport — it reaches a printer with its
//! own `fetch`, CORS permitting — but it must reach the *same conclusions*
//! about that printer as the server and the desktop app. So the UI fetches the
//! raw Moonraker responses and hands them here, rather than reimplementing the
//! rules in [`super::klipper`] a second time in TypeScript.

use serde::Serialize;
use wasm_bindgen::prelude::*;

use super::klipper::{looks_like_moonraker, KlipperProbe};

/// Interpret raw Moonraker responses as a
/// [`PrinterDetection`](super::detection::PrinterDetection).
///
/// `probes` is a JS object holding whichever replies the caller managed to
/// fetch, each the *parsed body* of its endpoint:
///
/// - `info` — `/printer/info`
/// - `toolhead` — `/printer/objects/query?toolhead`
/// - `objectList` — `/printer/objects/list`
/// - `bedMesh` — `/printer/objects/query?bed_mesh`
/// - `configfile` — `/printer/objects/query?configfile`
///
/// Every field is optional except `info`, which is what identifies the host as
/// Moonraker at all; without a convincing one this returns `null` so the caller
/// can fall through to its other probes (OctoPrint, PrusaLink).
#[wasm_bindgen(js_name = deriveKlipperDetection)]
pub fn derive_klipper_detection(probes: JsValue) -> Result<JsValue, JsValue> {
    let probes: serde_json::Value = serde_wasm_bindgen::from_value(probes)
        .map_err(|e| JsValue::from_str(&format!("invalid Moonraker probes: {e}")))?;

    let info = &probes["info"]["result"];
    if !looks_like_moonraker(info) {
        return Ok(JsValue::NULL);
    }

    let mut probe = KlipperProbe::default();
    probe.absorb_info(info);
    for stage in ["toolhead", "bedMesh", "configfile"] {
        probe.absorb_objects(&probes[stage]["result"]["status"]);
    }
    probe.absorb_object_list(&probes["objectList"]["result"]);

    // `serialize_maps_as_objects` is load-bearing, not a style choice. The
    // detection carries `serde_json::Value` bags (the derived params, and each
    // option's patch), and the default serializer turns a JSON object into a JS
    // `Map` — which spreads and `JSON.stringify`s to `{}`. Every derived
    // setting would silently vanish on the browser path alone.
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    probe
        .into_detection()
        .serialize(&serializer)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}
