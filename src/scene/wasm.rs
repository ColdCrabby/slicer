//! WebAssembly bindings for the unified scene engine.
//!
//! Exposes a [`SceneHandle`] that the Angular UI consumes via wasm-bindgen.
//! The same [`crate::scene::SceneState`] API is used here as in the CLI and
//! WS server — there is no parallel implementation.

use crate::mesh::types::Vertex;
use crate::scene::bed::BedConfig;
use crate::scene::loader::MeshFormat;
use crate::scene::ops::SceneOp;
use crate::scene::state::{ObjectId, SceneState};
use crate::scene::transform::Transform;
use js_sys::{Float32Array, Uint32Array};
#[cfg(feature = "web-slicer")]
use js_sys::{Function, Object, Reflect};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// Wire-format render buffers for one scene object.
///
/// Positions and normals are flat triples (`x, y, z`); indices reference
/// vertices in groups of three (one triangle per group).
#[wasm_bindgen]
pub struct RenderBuffer {
    positions: Float32Array,
    normals: Float32Array,
    indices: Uint32Array,
}

#[wasm_bindgen]
impl RenderBuffer {
    /// Vertex positions, flat `[x0, y0, z0, x1, y1, z1, …]`.
    #[wasm_bindgen(getter)]
    pub fn positions(&self) -> Float32Array {
        self.positions.clone()
    }

    /// Vertex normals, flat triples in the same vertex order as `positions`.
    #[wasm_bindgen(getter)]
    pub fn normals(&self) -> Float32Array {
        self.normals.clone()
    }

    /// Triangle indices, flat triples `[a0, b0, c0, a1, b1, c1, …]`.
    #[wasm_bindgen(getter)]
    pub fn indices(&self) -> Uint32Array {
        self.indices.clone()
    }
}

/// Wire-format scene op accepted by [`SceneHandle::apply_op`].
///
/// Mirrors the WS protocol's `SceneOpDto` minus the `Add` variant (handled by
/// the dedicated `add_mesh` method to keep raw bytes off the JSON path).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "args")]
pub enum SceneOpJs {
    Remove {
        id: u64,
    },
    RemoveMany {
        ids: Vec<u64>,
    },
    Duplicate {
        id: u64,
        #[serde(default)]
        offset: [f64; 3],
    },
    Translate {
        id: u64,
        delta: [f64; 3],
    },
    SetTransform {
        id: u64,
        translation: [f32; 3],
        euler_xyz_deg: [f32; 3],
        scale: [f32; 3],
    },
    Rotate {
        id: u64,
        axis: [f32; 3],
        degrees: f32,
    },
    Scale {
        id: u64,
        factors: [f32; 3],
    },
    CenterOnBed {
        id: u64,
    },
    DropToFloor {
        id: u64,
    },
    PlaceFaceOnFloor {
        id: u64,
        face_index: usize,
    },
    AutoOrient {
        id: u64,
        #[serde(default)]
        options: crate::orient::AutoOrientOptions,
    },
    ArrangeOnBed {
        ids: Vec<u64>,
        #[serde(default)]
        options: crate::orient::ArrangeOptions,
    },
}

/// JS-friendly snapshot of one scene object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneObjectJs {
    pub id: u64,
    pub name: String,
    pub translation: [f32; 3],
    pub euler_xyz_deg: [f32; 3],
    pub scale: [f32; 3],
    pub triangle_count: usize,
    pub world_aabb: [[f64; 3]; 2],
    /// Upload/file handle this object was loaded from, when known.
    pub source_id: Option<String>,
    /// Index of this object within its source file (0 for single-part files).
    pub source_part: usize,
    /// Part of the object falls outside the printable volume.
    pub out_of_bounds: bool,
    /// The object's footprint overlaps another object's.
    pub collides: bool,
}

/// JS-friendly snapshot of the bed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BedConfigJs {
    pub width: f64,
    pub depth: f64,
    pub height: f64,
    pub origin_offset_x: f64,
    pub origin_offset_y: f64,
    /// `"rectangular"` (default) or `"circular"`.
    #[serde(default)]
    pub shape: crate::scene::bed::BedShape,
}

/// JS-friendly full snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneSnapshotJs {
    pub objects: Vec<SceneObjectJs>,
    pub bed: BedConfigJs,
}

#[cfg(feature = "web-slicer")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliceResultJs {
    pub gcode: String,
    pub layer_count: usize,
}

impl From<&BedConfig> for BedConfigJs {
    fn from(b: &BedConfig) -> Self {
        Self {
            width: b.width,
            depth: b.depth,
            height: b.height,
            origin_offset_x: b.origin_offset_x,
            origin_offset_y: b.origin_offset_y,
            shape: b.shape,
        }
    }
}

/// Owned handle to a [`SceneState`] suitable for crossing the JS/WASM boundary.
#[wasm_bindgen]
pub struct SceneHandle {
    inner: SceneState,
}

#[wasm_bindgen]
impl SceneHandle {
    /// Create a new empty scene with the given bed configuration (JSON shape:
    /// `{ width, depth, height, origin_offset_x, origin_offset_y }`).
    #[wasm_bindgen(constructor)]
    pub fn new(bed: JsValue) -> Result<SceneHandle, JsValue> {
        console_error_panic_hook::set_once();
        let bed_js: BedConfigJs = serde_wasm_bindgen::from_value(bed)
            .map_err(|e| JsValue::from_str(&format!("invalid bed config: {}", e)))?;
        let bed = BedConfig {
            width: bed_js.width,
            depth: bed_js.depth,
            height: bed_js.height,
            origin_offset_x: bed_js.origin_offset_x,
            origin_offset_y: bed_js.origin_offset_y,
            shape: bed_js.shape,
        };
        Ok(SceneHandle {
            inner: SceneState::new(bed),
        })
    }

    /// Update the bed configuration in-place (same JSON shape as `new`).
    ///
    /// This preserves all objects/transforms and only changes how bed-aware
    /// operations (e.g. center/arrange) interpret the build volume.
    #[wasm_bindgen(js_name = setBed)]
    pub fn set_bed(&mut self, bed: JsValue) -> Result<(), JsValue> {
        let bed_js: BedConfigJs = serde_wasm_bindgen::from_value(bed)
            .map_err(|e| JsValue::from_str(&format!("invalid bed config: {}", e)))?;
        self.inner.bed = BedConfig {
            width: bed_js.width,
            depth: bed_js.depth,
            height: bed_js.height,
            origin_offset_x: bed_js.origin_offset_x,
            origin_offset_y: bed_js.origin_offset_y,
            shape: bed_js.shape,
        };
        Ok(())
    }

    /// Add a mesh from raw bytes. `format` must be `"stl"`, `"obj"`, or `"3mf"`.
    /// Returns the assigned object ids.
    ///
    /// A 3MF holding several parts yields **one id per part**, so a multi-model
    /// file lands on the plate as separate, individually selectable objects.
    /// STL and OBJ always yield exactly one.
    ///
    /// `source_id` is an opaque handle (typically the uploaded file's UUID)
    /// stored on every created object so the caller can later map them back to
    /// the bytes they came from. Pass `undefined` when there is nothing to
    /// correlate.
    #[wasm_bindgen(js_name = addMesh)]
    pub fn add_mesh(
        &mut self,
        name: String,
        format: &str,
        bytes: &[u8],
        source_id: Option<String>,
    ) -> Result<Vec<u64>, JsValue> {
        let format = MeshFormat::from_extension(format)
            .ok_or_else(|| JsValue::from_str(&format!("unknown mesh format '{}'", format)))?;
        let receipt = self
            .inner
            .apply(SceneOp::Add {
                name,
                format,
                bytes: bytes.to_vec(),
                source_id,
            })
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        // The Add op records its inverse as RemoveMany { ids }; pluck them back out.
        match receipt.inverse {
            SceneOp::RemoveMany { ids } => Ok(ids.into_iter().map(|id| id.0).collect()),
            _ => unreachable!("Add op always returns a RemoveMany inverse"),
        }
    }

    /// Apply a single scene op. Pass a JSON value matching [`SceneOpJs`].
    #[wasm_bindgen(js_name = applyOp)]
    pub fn apply_op(&mut self, op: JsValue) -> Result<(), JsValue> {
        let op_js: SceneOpJs = serde_wasm_bindgen::from_value(op)
            .map_err(|e| JsValue::from_str(&format!("invalid op: {}", e)))?;
        let op = js_to_op(op_js);
        self.inner
            .apply(op)
            .map(|_| ())
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Apply a single scene op with optional modifiers (e.g. heavy gravity).
    /// `options` JSON shape: `{ gravity?: boolean }`. Pass `null`/`undefined`
    /// to use defaults.
    #[wasm_bindgen(js_name = applyOpWithOptions)]
    pub fn apply_op_with_options(&mut self, op: JsValue, options: JsValue) -> Result<(), JsValue> {
        let op_js: SceneOpJs = serde_wasm_bindgen::from_value(op)
            .map_err(|e| JsValue::from_str(&format!("invalid op: {}", e)))?;
        let opts: crate::scene::SceneOptions = if options.is_null() || options.is_undefined() {
            crate::scene::SceneOptions::default()
        } else {
            serde_wasm_bindgen::from_value(options)
                .map_err(|e| JsValue::from_str(&format!("invalid options: {}", e)))?
        };
        let op = js_to_op(op_js);
        self.inner
            .apply_with_options(op, opts)
            .map(|_| ())
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Get the render buffer for an object.
    #[wasm_bindgen(js_name = getRenderBuffer)]
    pub fn get_render_buffer(&self, id: u64) -> Result<RenderBuffer, JsValue> {
        let obj = self
            .inner
            .get(ObjectId(id))
            .ok_or_else(|| JsValue::from_str(&format!("object {} not found", id)))?;
        let mesh = obj.mesh.as_ref();

        // Flatten faces into independent triangles; one normal per vertex.
        // This gives flat shading and avoids needing a vertex de-dup pass on
        // the JS side.
        let face_count = mesh.faces.len();
        let mut positions = Vec::with_capacity(face_count * 9);
        let mut normals = Vec::with_capacity(face_count * 9);
        let mut indices = Vec::with_capacity(face_count * 3);

        for (face_idx, face) in mesh.faces.iter().enumerate() {
            let n = face_normal(face);
            for v in &face.vertices {
                positions.push(v.x as f32);
                positions.push(v.y as f32);
                positions.push(v.z as f32);
                normals.push(n[0]);
                normals.push(n[1]);
                normals.push(n[2]);
            }
            let base = (face_idx * 3) as u32;
            indices.push(base);
            indices.push(base + 1);
            indices.push(base + 2);
        }

        Ok(RenderBuffer {
            positions: Float32Array::from(positions.as_slice()),
            normals: Float32Array::from(normals.as_slice()),
            indices: Uint32Array::from(indices.as_slice()),
        })
    }

    /// 4×4 transform matrix for an object as 16 column-major floats.
    #[wasm_bindgen(js_name = getMatrix)]
    pub fn get_matrix(&self, id: u64) -> Result<Float32Array, JsValue> {
        let obj = self
            .inner
            .get(ObjectId(id))
            .ok_or_else(|| JsValue::from_str(&format!("object {} not found", id)))?;
        let m = obj.transform.to_matrix().to_cols_array();
        Ok(Float32Array::from(m.as_slice()))
    }

    /// Coplanar face groups for the mesh identified by `id`.
    ///
    /// Returns a `Uint32Array` of length `face_count` where each element is the
    /// group id of that face (0-based contiguous integers). Faces with the same
    /// group id are coplanar and share at least one edge. Adjacent triangles on
    /// a flat bottom of a model will all share a single group id; the triangles
    /// of the raised lettering will form distinct groups.
    ///
    /// `angle_threshold_deg` (recommended: 1.0) controls how close two normals
    /// must be for faces to merge. Use a larger value (e.g. 5.0) for slightly
    /// uneven surfaces; use 0.1 for strict planarity.
    #[wasm_bindgen(js_name = getFaceGroups)]
    pub fn get_face_groups(
        &self,
        id: u64,
        angle_threshold_deg: f32,
    ) -> Result<Uint32Array, JsValue> {
        let obj = self
            .inner
            .get(ObjectId(id))
            .ok_or_else(|| JsValue::from_str(&format!("object {} not found", id)))?;
        let groups = crate::mesh::analysis::compute_coplanar_groups(
            obj.mesh.as_ref(),
            angle_threshold_deg,
            0.001, // 1 µm merge tolerance — tight enough for any FDM print mesh
        );
        Ok(Uint32Array::from(groups.as_slice()))
    }

    /// Full scene snapshot suitable for driving Angular signals.
    #[wasm_bindgen]
    pub fn snapshot(&self) -> Result<JsValue, JsValue> {
        // Placement is O(n²) over objects but n is the number of models on a
        // plate (tens at most), and this runs once per snapshot rather than
        // per frame.
        let placement = self.inner.placement_report();
        let snap = SceneSnapshotJs {
            objects: self
                .inner
                .objects
                .iter()
                .zip(placement.iter())
                .map(|(o, p)| {
                    let world = o.world_aabb();
                    SceneObjectJs {
                        id: o.id.0,
                        name: o.name.clone(),
                        translation: o.transform.translation,
                        euler_xyz_deg: o.transform.to_euler_xyz_deg(),
                        scale: o.transform.scale,
                        triangle_count: o.mesh.faces.len(),
                        world_aabb: [
                            [world.min.x, world.min.y, world.min.z],
                            [world.max.x, world.max.y, world.max.z],
                        ],
                        source_id: o.source_id.clone(),
                        source_part: o.source_part,
                        out_of_bounds: p.out_of_bounds,
                        collides: p.collides,
                    }
                })
                .collect(),
            bed: BedConfigJs::from(&self.inner.bed),
        };
        serde_wasm_bindgen::to_value(&snap).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Slice the current scene in-browser and return the generated G-code.
    ///
    /// This method is only available in the alternative `web-slicer` wasm
    /// build because it pulls in the full polygon clipping and slicing stack.
    #[cfg(feature = "web-slicer")]
    #[wasm_bindgen(js_name = sliceGcode)]
    pub fn slice_gcode(&self, params: JsValue) -> Result<JsValue, JsValue> {
        self.slice_gcode_impl(params, None)
    }

    /// Slice the current scene in-browser and report log/phase/progress events
    /// through a JavaScript callback while the synchronous WASM call runs.
    ///
    /// `callback` receives objects shaped like:
    /// `{ type: "log", level, message }`,
    /// `{ type: "phase", phase, event, elapsed_ms? }`, and
    /// `{ type: "progress", current_layer, total_layers }`.
    #[cfg(feature = "web-slicer")]
    #[wasm_bindgen(js_name = sliceGcodeWithEvents)]
    pub fn slice_gcode_with_events(
        &self,
        params: JsValue,
        callback: Function,
    ) -> Result<JsValue, JsValue> {
        self.slice_gcode_impl(params, Some(callback))
    }
}

#[cfg(feature = "web-slicer")]
impl SceneHandle {
    fn slice_gcode_impl(
        &self,
        params: JsValue,
        callback: Option<Function>,
    ) -> Result<JsValue, JsValue> {
        let params: crate::settings::params::SlicingParams = serde_wasm_bindgen::from_value(params)
            .map_err(|e| JsValue::from_str(&format!("invalid slicing params: {}", e)))?;

        if self.inner.objects.is_empty() {
            return Err(JsValue::from_str(
                "scene is empty; add at least one object before slicing",
            ));
        }

        // Keep the plate's objects apart and let `slice_plate` decide whether
        // it can merge them — the browser slicer honours exclude-object and
        // sequential printing exactly like the CLI and the server do.
        let plate_objects: Vec<crate::core::ObjectInput> = self
            .inner
            .objects
            .iter()
            .map(|object| {
                crate::core::ObjectInput::new(
                    object.name.clone(),
                    crate::scene::apply_transform(object.mesh.as_ref(), &object.transform),
                )
            })
            .collect();

        if plate_objects.iter().all(|o| o.mesh.faces.is_empty()) {
            return Err(JsValue::from_str(
                "combined scene has no triangles; nothing to slice",
            ));
        }

        let logger = WasmSliceLogger::new(callback);
        let plate = crate::core::slice_plate(&plate_objects, &params, &logger);
        let layer_count = plate.layers.len();
        logger.emit_progress(layer_count, layer_count);

        let t_gcode =
            crate::logging::PhaseTimer::start(crate::logging::phases::GCODE_GENERATION, &logger);
        let result = SliceResultJs {
            layer_count,
            gcode: crate::gcode::generate_gcode_for_plate(&plate, &params),
        };
        t_gcode.finish();

        serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
    }
}

#[cfg(feature = "web-slicer")]
struct WasmSliceLogger {
    callback: Option<Function>,
    /// Current object scope `(index, count)`, both 1-based, or `(0, 0)` for a
    /// single merged slice. `Cell` is enough — the logger runs synchronously on
    /// one worker thread (see the `unsafe impl Sync` note below).
    object_scope: std::cell::Cell<(u32, u32)>,
}

#[cfg(feature = "web-slicer")]
impl WasmSliceLogger {
    fn new(callback: Option<Function>) -> Self {
        Self {
            callback,
            object_scope: std::cell::Cell::new((0, 0)),
        }
    }

    fn emit_log(&self, level: &str, message: &str) {
        let event = Object::new();
        set_js_str(&event, "type", "log");
        set_js_str(&event, "level", level);
        set_js_str(&event, "message", message);
        self.emit(event);
    }

    fn emit_phase(&self, phase: &str, event_name: &str, elapsed_ms: Option<u64>) {
        let event = Object::new();
        set_js_str(&event, "type", "phase");
        set_js_str(&event, "phase", phase);
        set_js_str(&event, "event", event_name);
        if let Some(elapsed_ms) = elapsed_ms {
            set_js_num(&event, "elapsed_ms", elapsed_ms as f64);
        }
        let (object, count) = self.object_scope.get();
        if count > 0 {
            set_js_num(&event, "object", object as f64);
            set_js_num(&event, "object_count", count as f64);
        }
        self.emit(event);
    }

    fn emit_progress(&self, current_layer: usize, total_layers: usize) {
        let event = Object::new();
        set_js_str(&event, "type", "progress");
        set_js_num(&event, "current_layer", current_layer as f64);
        set_js_num(&event, "total_layers", total_layers as f64);
        self.emit(event);
    }

    fn emit(&self, event: Object) {
        if let Some(callback) = &self.callback {
            let _ = callback.call1(&JsValue::NULL, &event.into());
        }
    }
}

#[cfg(feature = "web-slicer")]
impl crate::logging::ProcessLogger for WasmSliceLogger {
    fn log_info(&self, msg: &str) {
        self.emit_log("info", msg);
    }

    fn log_debug(&self, msg: &str) {
        self.emit_log("debug", msg);
    }

    fn log_warn(&self, msg: &str) {
        self.emit_log("warn", msg);
    }

    fn log_phase_start(&self, phase: &str) {
        self.emit_phase(phase, "start", None);
    }

    fn log_phase_end(&self, phase: &str, elapsed_ms: u64) {
        self.emit_phase(phase, "end", Some(elapsed_ms));
    }

    fn set_object_scope(&self, index: usize, count: usize) {
        self.object_scope.set((index as u32, count as u32));
    }

    fn clear_object_scope(&self) {
        self.object_scope.set((0, 0));
    }
}

// The callback is invoked synchronously on the worker thread that owns the
// WASM instance. The logger only needs Send/Sync to satisfy ProcessLogger's
// cross-target trait bounds; it is not shared across browser threads.
#[cfg(feature = "web-slicer")]
unsafe impl Send for WasmSliceLogger {}
#[cfg(feature = "web-slicer")]
unsafe impl Sync for WasmSliceLogger {}

#[cfg(feature = "web-slicer")]
fn set_js_str(object: &Object, key: &str, value: &str) {
    let _ = Reflect::set(object, &JsValue::from_str(key), &JsValue::from_str(value));
}

#[cfg(feature = "web-slicer")]
fn set_js_num(object: &Object, key: &str, value: f64) {
    let _ = Reflect::set(object, &JsValue::from_str(key), &JsValue::from_f64(value));
}

fn js_to_op(op: SceneOpJs) -> SceneOp {
    match op {
        SceneOpJs::Remove { id } => SceneOp::Remove { id: ObjectId(id) },
        SceneOpJs::RemoveMany { ids } => SceneOp::RemoveMany {
            ids: ids.into_iter().map(ObjectId).collect(),
        },
        SceneOpJs::Duplicate { id, offset } => SceneOp::Duplicate {
            id: ObjectId(id),
            offset,
        },
        SceneOpJs::Translate { id, delta } => SceneOp::Translate {
            id: ObjectId(id),
            delta,
        },
        SceneOpJs::SetTransform {
            id,
            translation,
            euler_xyz_deg,
            scale,
        } => SceneOp::SetTransform {
            id: ObjectId(id),
            transform: Transform::from_euler_xyz_deg(translation, euler_xyz_deg, scale),
        },
        SceneOpJs::Rotate { id, axis, degrees } => SceneOp::Rotate {
            id: ObjectId(id),
            axis,
            radians: degrees.to_radians(),
        },
        SceneOpJs::Scale { id, factors } => SceneOp::Scale {
            id: ObjectId(id),
            factors,
        },
        SceneOpJs::CenterOnBed { id } => SceneOp::CenterOnBed { id: ObjectId(id) },
        SceneOpJs::DropToFloor { id } => SceneOp::DropToFloor { id: ObjectId(id) },
        SceneOpJs::PlaceFaceOnFloor { id, face_index } => SceneOp::PlaceFaceOnFloor {
            id: ObjectId(id),
            face_index,
        },
        SceneOpJs::AutoOrient { id, options } => SceneOp::AutoOrient {
            id: ObjectId(id),
            options,
        },
        SceneOpJs::ArrangeOnBed { ids, options } => SceneOp::ArrangeOnBed {
            ids: ids.into_iter().map(ObjectId).collect(),
            options,
        },
    }
}

fn face_normal(face: &crate::mesh::types::Face) -> [f32; 3] {
    if let Some(n) = face.normal {
        let len = (n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
        if len > 1e-12 {
            return [(n.x / len) as f32, (n.y / len) as f32, (n.z / len) as f32];
        }
    }
    let a = &face.vertices[0];
    let b = &face.vertices[1];
    let c = &face.vertices[2];
    let ux = b.x - a.x;
    let uy = b.y - a.y;
    let uz = b.z - a.z;
    let vx = c.x - a.x;
    let vy = c.y - a.y;
    let vz = c.z - a.z;
    let nx = uy * vz - uz * vy;
    let ny = uz * vx - ux * vz;
    let nz = ux * vy - uy * vx;
    let len = (nx * nx + ny * ny + nz * nz).sqrt();
    if len > 1e-12 {
        [(nx / len) as f32, (ny / len) as f32, (nz / len) as f32]
    } else {
        [0.0, 0.0, 1.0]
    }
}

#[allow(dead_code)]
fn _unused_vertex(_v: Vertex) {}
