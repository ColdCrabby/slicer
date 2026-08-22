use js_sys::Float32Array;
use wasm_bindgen::prelude::*;

use super::parser::parse_gcode_bytes;
use super::types::InternalLayer;

// ── GcodeLayerBuffer ────────────────────────────────────────────────────────

#[wasm_bindgen]
pub struct GcodeLayerBuffer {
    z: f32,
    blocks_roles: Vec<u8>,
    blocks_data: Vec<Float32Array>,
    nozzle_temp: f32,
    tool: u32,
    layer_time_s: f32,
    fan_keys: Vec<String>,
    fan_speeds: Vec<f32>,
}

#[wasm_bindgen]
impl GcodeLayerBuffer {
    /// Z coordinate of this layer.
    #[wasm_bindgen(getter)]
    pub fn z(&self) -> f32 {
        self.z
    }

    #[wasm_bindgen(js_name = blocksCount)]
    pub fn blocks_count(&self) -> usize {
        self.blocks_roles.len()
    }

    #[wasm_bindgen(js_name = blockRole)]
    pub fn block_role(&self, i: usize) -> u8 {
        self.blocks_roles[i]
    }

    #[wasm_bindgen(js_name = blockData)]
    pub fn block_data(&self, i: usize) -> Float32Array {
        self.blocks_data[i].clone()
    }

    /// Nozzle target temperature (°C) active on this layer; `0.0` when unknown.
    #[wasm_bindgen(js_name = nozzleTemp)]
    pub fn nozzle_temp(&self) -> f32 {
        self.nozzle_temp
    }

    /// Active tool / extruder index for this layer.
    #[wasm_bindgen(getter)]
    pub fn tool(&self) -> u32 {
        self.tool
    }

    /// Layer print time (seconds) from a `;LAYER_TIME:` marker; `0.0` when absent.
    #[wasm_bindgen(js_name = layerTimeS)]
    pub fn layer_time_s(&self) -> f32 {
        self.layer_time_s
    }

    /// Number of fans with a recorded speed on this layer.
    #[wasm_bindgen(js_name = fanCount)]
    pub fn fan_count(&self) -> usize {
        self.fan_keys.len()
    }

    /// Stable key of the `i`-th fan (`"P0"`, `"P2"`, or a Klipper fan name).
    #[wasm_bindgen(js_name = fanKey)]
    pub fn fan_key(&self, i: usize) -> String {
        self.fan_keys.get(i).cloned().unwrap_or_default()
    }

    /// Speed (`0.0..=1.0`) of the `i`-th fan on this layer.
    #[wasm_bindgen(js_name = fanSpeed)]
    pub fn fan_speed(&self, i: usize) -> f32 {
        self.fan_speeds.get(i).copied().unwrap_or(0.0)
    }
}

fn into_float32_array(data: &[f32]) -> Float32Array {
    Float32Array::from(data)
}

fn layer_to_buffer(layer: &InternalLayer) -> GcodeLayerBuffer {
    let mut roles = Vec::with_capacity(layer.blocks.len());
    let mut data = Vec::with_capacity(layer.blocks.len());
    for b in &layer.blocks {
        roles.push(b.role.id());
        data.push(into_float32_array(&b.data));
    }
    let mut fan_keys = Vec::with_capacity(layer.meta.fans.len());
    let mut fan_speeds = Vec::with_capacity(layer.meta.fans.len());
    for fan in &layer.meta.fans {
        fan_keys.push(fan.key.clone());
        fan_speeds.push(fan.speed);
    }
    GcodeLayerBuffer {
        z: layer.z,
        blocks_roles: roles,
        blocks_data: data,
        nozzle_temp: layer.meta.nozzle_temp.unwrap_or(0.0),
        tool: layer.meta.tool,
        layer_time_s: layer.meta.layer_time_s.unwrap_or(0.0),
        fan_keys,
        fan_speeds,
    }
}

// ── GcodeHandle ─────────────────────────────────────────────────────────────

/// Owned handle over all parsed layers of a GCode file.
///
/// ```js
/// const handle = GcodeHandle.parse(new Uint8Array(bytes));
/// console.log(handle.layerCount());   // total layers
/// const layer = handle.getLayer(5);   // GcodeLayerBuffer
/// ```
#[wasm_bindgen]
pub struct GcodeHandle {
    layers: Vec<InternalLayer>,
}

#[wasm_bindgen]
impl GcodeHandle {
    /// Parse a complete GCode file from raw bytes.
    ///
    /// Accepts both UTF-8 and ASCII. Invalid byte sequences are replaced with
    /// the Unicode replacement character (`U+FFFD`).
    #[wasm_bindgen]
    pub fn parse(bytes: &[u8]) -> GcodeHandle {
        console_error_panic_hook::set_once();
        GcodeHandle {
            layers: parse_gcode_bytes(bytes),
        }
    }

    /// Total number of layers detected in the file.
    #[wasm_bindgen(js_name = layerCount)]
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// Z coordinate for the layer at `index`. Returns `0.0` for out-of-bounds.
    #[wasm_bindgen(js_name = layerZ)]
    pub fn layer_z(&self, index: usize) -> f32 {
        self.layers.get(index).map(|l| l.z).unwrap_or(0.0)
    }

    /// Geometry buffers for the layer at `index`.
    ///
    /// Returns a `JsValue` error if `index >= layer_count()`.
    #[wasm_bindgen(js_name = getLayer)]
    pub fn get_layer(&self, index: usize) -> Result<GcodeLayerBuffer, JsValue> {
        self.layers.get(index).map(layer_to_buffer).ok_or_else(|| {
            JsValue::from_str(&format!(
                "layer index {index} out of range (layer_count = {})",
                self.layers.len()
            ))
        })
    }
}
