//! WebSocket protocol message types shared between the server and the browser.
//!
//! **All** browser ↔ server communication goes over a single `/ws` endpoint.
//! Messages are JSON objects with a discriminant `"type"` field (snake_case).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::scene::MeshFormat;
use crate::settings::params::SlicingParams;

/// Summary of a completed slicing session for history/re-download.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionSummary {
    /// Unique request identifier
    pub request_uuid: String,
    /// Original uploaded filename
    pub original_filename: Option<String>,
    /// Number of layers in the sliced G-code
    pub layer_count: Option<usize>,
    /// Session creation timestamp (RFC3339)
    pub created_at: String,
    /// URL to download the G-code file
    pub download_url: String,
}

/// Affine transform encoded for the protocol boundary using Euler-XYZ degrees.
///
/// Mirrors the `from_euler_xyz_deg` view of [`crate::scene::Transform`] so
/// payloads stay human-readable JSON. Defaults to the identity transform so
/// callers may omit any field.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TransformDto {
    /// Translation in millimeters.
    #[serde(default = "TransformDto::default_zero3")]
    pub translation: [f32; 3],
    /// Rotation as intrinsic Euler-XYZ angles in **degrees**.
    #[serde(default = "TransformDto::default_zero3")]
    pub euler_xyz_deg: [f32; 3],
    /// Per-axis scale factors.
    #[serde(default = "TransformDto::default_one3")]
    pub scale: [f32; 3],
}

impl TransformDto {
    fn default_zero3() -> [f32; 3] {
        [0.0; 3]
    }
    fn default_one3() -> [f32; 3] {
        [1.0; 3]
    }
}

impl Default for TransformDto {
    fn default() -> Self {
        Self {
            translation: Self::default_zero3(),
            euler_xyz_deg: Self::default_zero3(),
            scale: Self::default_one3(),
        }
    }
}

/// One placed object in a slice request: which uploaded mesh to use and the
/// transform (translation / rotation / scale) the frontend currently has
/// applied to it.
///
/// `file_id` is the **file UUID** returned by `POST /api/upload` in the
/// `ofids` list — distinct from the workplate's `ruuid`. The server resolves
/// the file (including its on-disk extension) from the database, so callers
/// don't need to — and should not — encode the format here.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SceneObjectSliceDto {
    /// File identifier from `ofids` in the upload response.
    pub file_id: String,
    /// Which object *within* that file this refers to (0-based, build order).
    ///
    /// A 3MF holds a whole scene, so one upload can back several plate
    /// objects. Without this the server would re-load the entire file for
    /// each of them and print every part once per part.
    #[serde(default)]
    pub part_index: usize,
    /// Transform to bake into the mesh before slicing.
    #[serde(default)]
    pub transform: TransformDto,
}

/// Euler-XYZ degrees and a `file_id` reference for `Add` so payloads stay
/// human-readable JSON.
///
/// `file_id` is the upload `request_uuid` returned by `POST /api/upload`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", content = "args")]
pub enum SceneOpDto {
    /// Add a mesh by reference to a previously-uploaded file.
    Add {
        name: String,
        format: MeshFormat,
        file_id: String,
    },
    /// Remove an object by id.
    Remove { id: u64 },
    /// Remove several objects at once (the inverse of adding a multi-part file).
    RemoveMany { ids: Vec<u64> },
    /// Clone an object, sharing the original's mesh and source file.
    ///
    /// `offset` (scene mm) nudges the copy so it does not land exactly on
    /// top of the original.
    Duplicate {
        id: u64,
        #[serde(default)]
        offset: [f64; 3],
    },
    /// Translate by `[x, y, z]` mm.
    Translate { id: u64, delta: [f64; 3] },
    /// Replace the full transform: translation (mm), Euler-XYZ degrees, scale.
    SetTransform {
        id: u64,
        translation: [f32; 3],
        euler_xyz_deg: [f32; 3],
        scale: [f32; 3],
    },
    /// Rotate around `axis` by `degrees`, composed with the existing rotation.
    Rotate {
        id: u64,
        axis: [f32; 3],
        degrees: f32,
    },
    /// Multiply per-axis scale by `factors`.
    Scale { id: u64, factors: [f32; 3] },
    /// Center the object on the bed in XY (preserves Z).
    CenterOnBed { id: u64 },
    /// Drop the object so its lowest Z vertex sits on Z=0.
    DropToFloor { id: u64 },
    /// Rotate so the chosen face's normal points down, then place that face
    /// on the floor (z = 0). Replaces the legacy `align_face_to_floor` op.
    PlaceFaceOnFloor { id: u64, face_index: usize },
    /// Automatically rotate the object to minimise overhangs and maximise
    /// flat bed-contact area, then drop it to the floor.
    AutoOrient {
        id: u64,
        #[serde(default)]
        options: crate::orient::AutoOrientOptions,
    },
    /// Auto-orient and arrange multiple objects on the bed without overlap.
    ///
    /// Orients each listed object (when `options.auto_orient` is `true`),
    /// then packs them using a shelf-first-fit algorithm with
    /// `options.spacing_mm` between objects, and centers the result on the
    /// bed.
    ArrangeOnBed {
        ids: Vec<u64>,
        #[serde(default)]
        options: crate::orient::ArrangeOptions,
    },
}

/// Optional modifiers applied to every op in a [`ClientMessage::Scene`] batch.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
pub struct SceneOptionsDto {
    /// "Heavy gravity": after each transforming op, drop the affected object
    /// to the floor (`world_aabb().min.z = 0`). No effect on `Add`, `Remove`,
    /// `DropToFloor`, or `PlaceFaceOnFloor`.
    #[serde(default)]
    pub gravity: bool,
}

/// Snapshot of a scene object sent to the client (no mesh data).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SceneObjectDto {
    pub id: u64,
    pub name: String,
    pub translation: [f32; 3],
    pub euler_xyz_deg: [f32; 3],
    pub scale: [f32; 3],
    /// Number of triangle faces in the mesh.
    pub triangle_count: usize,
    /// World-space AABB after applying the current transform: `[min, max]`.
    pub world_aabb: [[f64; 3]; 2],
}

/// Snapshot of the bed configuration sent to the client.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BedConfigDto {
    pub width: f64,
    pub depth: f64,
    pub height: f64,
    pub origin_offset_x: f64,
    pub origin_offset_y: f64,
}

/// Messages sent **from the browser to the server**.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum ClientMessage {
    /// Start a slice job.
    ///
    /// Every slice carries:
    ///
    /// - **`request_uuid`**: the workplate / scene UUID (`ruuid` from the
    ///   upload response). Used to track the resulting G-code download.
    /// - **`scene`**: the placed-object scene the user has built up in the
    ///   viewer. Each entry references an uploaded file by `file_id` (a
    ///   `file_uuid` from `ofids` — *not* the workplate UUID). The server
    ///   resolves the file via the DB (so it picks the right loader from the
    ///   on-disk extension), bakes every transform via `scene::apply_transform`,
    ///   and merges the results into a single mesh before `process_mesh`.
    /// - **either `profiles` or `settings`** — the parameters to slice with:
    ///   - `profiles`: the preferred, structured form. The active printer /
    ///     filament / process profiles plus a sparse `overrides` diff; the
    ///     server resolves them via [`crate::profiles::resolve`]. This is the
    ///     single source of truth going forward — the engine owns the profile
    ///     definitions and composition rules.
    ///   - `settings`: the legacy pre-flattened [`SlicingParams`] blob, kept
    ///     for backward compatibility while the UI migrates. Ignored when
    ///     `profiles` is present.
    ///
    /// At least one of `profiles`/`settings` must be present; when neither is,
    /// engine defaults are used.
    ///
    /// There is no longer a legacy "slice the upload as-is" fallback — the
    /// scene is the single source of truth for what gets sliced.
    Slice {
        /// Workplate UUID (the `ruuid` from `POST /api/upload`). Also the
        /// key the resulting G-code is stored against.
        request_uuid: String,
        /// Placed-object scene. Must contain at least one entry.
        scene: Vec<SceneObjectSliceDto>,
        /// Structured profile selection + user override diff (preferred).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profiles: Option<Box<crate::profiles::ProfileSelection>>,
        /// Legacy pre-flattened parameters (used only when `profiles` is absent).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        settings: Option<Box<SlicingParams>>,
    },
    /// Request a list of previously completed slicing sessions.
    ListSessions,
    /// Abort / reset the current state.
    Reset,
    /// Apply one or more scene operations in order. Server replies with
    /// [`ServerMessage::SceneState`] on success.
    Scene {
        ops: Vec<SceneOpDto>,
        /// Optional batch-level modifiers (e.g. heavy gravity).
        #[serde(default)]
        options: SceneOptionsDto,
    },
    /// Request the current scene snapshot. Server replies with
    /// [`ServerMessage::SceneState`].
    SceneSnapshot,
    /// Probe a printer connection and report its live status.
    ///
    /// The server performs the HTTP request on the client's behalf so the
    /// probe is **not subject to browser CORS** (Moonraker ships no permissive
    /// CORS headers). `printer_id` is the UI's profile id, echoed back in the
    /// [`ServerMessage::PrinterStatus`] reply so the browser can correlate the
    /// response with the right card.
    CheckPrinter {
        printer_id: String,
        connection: crate::profiles::PrinterConnection,
    },
    /// Probe a single URL and report everything we can learn about the printer
    /// (kind, bed volume, nozzle, kinematics) so the setup wizard can prefill
    /// itself. Runs server-side to sidestep browser CORS. Replies with
    /// [`ServerMessage::PrinterDetected`].
    DetectPrinter {
        /// Host / address the user typed (bare host, `host:port`, or full URL).
        host: String,
    },
    /// Upload the G-code previously sliced for `request_uuid` to a printer,
    /// optionally starting the print. Replies with
    /// [`ServerMessage::PrinterSendResult`].
    SendToPrinter {
        /// Workplate UUID whose sliced G-code should be sent.
        request_uuid: String,
        /// UI profile id, echoed back for correlation.
        printer_id: String,
        /// Target printer connection details.
        connection: crate::profiles::PrinterConnection,
        /// Filename to store on the printer (defaults to `<uuid>.gcode`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        /// Start the print immediately after upload.
        #[serde(default)]
        start: bool,
    },
}

/// Messages sent **from the server to the browser**.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum ServerMessage {
    /// Sent once immediately after the WebSocket handshake completes.
    Connected { version: String },
    /// A log line for the status panel.
    Log { level: String, message: String },
    /// A performance timing marker for a pipeline phase.
    ///
    /// Emitted at the start and end of each major processing step so the
    /// browser can display elapsed times in the status panel.
    PhaseMarker {
        /// Pipeline phase name (see `slicer_engine::logging::phases`).
        phase: String,
        /// `"start"` when the phase begins; `"end"` when it completes.
        event: String,
        /// Elapsed time in milliseconds. Only present when `event` is `"end"`.
        #[serde(skip_serializing_if = "Option::is_none")]
        elapsed_ms: Option<u64>,
        /// 1-based index of the object this phase belongs to, on a plate sliced
        /// object-by-object. Absent for a single merged slice.
        #[serde(skip_serializing_if = "Option::is_none")]
        object: Option<u32>,
        /// Total number of objects being sliced individually. Absent for a
        /// single merged slice. Lets the UI show "(2 of 3)" and keep progress
        /// moving forward across per-object pipeline restarts.
        #[serde(skip_serializing_if = "Option::is_none")]
        object_count: Option<u32>,
    },
    /// Incremental slicing progress.
    Progress {
        current_layer: usize,
        total_layers: usize,
    },
    /// Slice finished successfully. Download the G-code from the provided URL.
    SliceComplete {
        layer_count: usize,
        /// HTTP GET this URL to download the generated G-code file
        download_url: String,
    },
    /// List of previously completed slicing sessions.
    SessionsList { sessions: Vec<SessionSummary> },
    /// Snapshot of the per-session scene state.
    SceneState {
        objects: Vec<SceneObjectDto>,
        bed: BedConfigDto,
    },
    /// Live status of a printer connection (reply to
    /// [`ClientMessage::CheckPrinter`]).
    PrinterStatus {
        /// Echoes the `printer_id` from the request.
        printer_id: String,
        /// The host answered a status query.
        online: bool,
        /// Firmware/host state (`ready`, `error`, `startup`, `shutdown`, …).
        #[serde(skip_serializing_if = "Option::is_none")]
        state: Option<String>,
        /// Current job state (`standby`, `printing`, `paused`, `complete`, …).
        #[serde(skip_serializing_if = "Option::is_none")]
        print_state: Option<String>,
        /// Print progress in `0.0..=1.0` when a job is active.
        #[serde(skip_serializing_if = "Option::is_none")]
        progress: Option<f32>,
        /// Human-readable detail (an error reason when offline).
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// Result of a [`ClientMessage::SendToPrinter`] request.
    PrinterSendResult {
        printer_id: String,
        request_uuid: String,
        ok: bool,
        message: String,
        started: bool,
    },
    /// Result of a [`ClientMessage::DetectPrinter`] probe. Every hardware field
    /// is optional — detection is best-effort. When `reachable` is false only
    /// `message` is meaningful.
    PrinterDetected {
        /// Echoes the probed host so the client can correlate the reply.
        host: String,
        /// The host answered at least one probe.
        reachable: bool,
        /// Detected transport (`moonraker`, `octoprint`, `prusalink`, or
        /// `none` when nothing answered).
        kind: crate::profiles::PrinterConnectionKind,
        /// Human-readable summary (a success note or the failure reason).
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        /// Friendly name (e.g. Klipper hostname), when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Model designation, when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// Manufacturer / firmware family, when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        vendor: Option<String>,
        /// G-code dialect the firmware speaks (`marlin`, `klipper`).
        #[serde(skip_serializing_if = "Option::is_none")]
        firmware: Option<String>,
        /// Bed shape (`rectangular`, `circular`), when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        bed_shape: Option<crate::profiles::printer::BedShape>,
        /// Bed width / diameter (mm), when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        bed_width: Option<f64>,
        /// Bed depth (mm), when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        bed_depth: Option<f64>,
        /// Max Z height (mm), when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        bed_height: Option<f64>,
        /// True for delta / center-origin machines, when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        origin_at_center: Option<bool>,
        /// Nozzle diameter (mm), when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        nozzle_diameter_mm: Option<f64>,
    },
    /// The engine's profile library changed on disk (another client/tab edited
    /// a category). Clients should refetch `GET /api/profiles` for `kind`.
    ///
    /// `kind` is the lowercase category token (`printers`, `filaments`,
    /// `processes`, `labels`) — matches [`crate::profiles::ProfileKind::as_str`].
    ProfilesChanged { kind: String },
    /// A fatal error occurred during processing.
    Error { message: String },
}

impl ServerMessage {
    /// Convenience constructor for an `info`-level log message.
    pub fn log_info(message: impl Into<String>) -> Self {
        Self::Log {
            level: "info".to_string(),
            message: message.into(),
        }
    }

    /// Convenience constructor for an `error` message.
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The slice request must round-trip through serde with an explicit
    /// `scene` list so the server can honour user-applied transforms.
    /// `format` is **not** part of the wire shape — the server resolves the
    /// loader from the upload's stored extension.
    #[test]
    fn slice_message_with_scene_round_trips() {
        let json = r#"{
            "type": "Slice",
            "request_uuid": "00000000-0000-0000-0000-000000000001",
            "scene": [{
                "file_id": "00000000-0000-0000-0000-000000000010",
                "transform": {
                    "translation": [10.0, 20.0, 0.0],
                    "euler_xyz_deg": [0.0, 0.0, 90.0],
                    "scale": [1.0, 1.0, 1.0]
                }
            }],
            "settings": {}
        }"#;
        let parsed: ClientMessage = serde_json::from_str(json).expect("parse");
        match parsed {
            ClientMessage::Slice { scene, .. } => {
                assert_eq!(scene.len(), 1);
                assert_eq!(scene[0].file_id, "00000000-0000-0000-0000-000000000010");
                assert_eq!(scene[0].transform.translation, [10.0, 20.0, 0.0]);
                assert_eq!(scene[0].transform.euler_xyz_deg, [0.0, 0.0, 90.0]);
            }
            _ => panic!("expected Slice with scene"),
        }
    }

    /// A `Slice` without `scene` must fail to parse — there is no longer a
    /// legacy single-upload fallback.
    #[test]
    fn slice_message_without_scene_is_rejected() {
        let json = r#"{
            "type": "Slice",
            "request_uuid": "00000000-0000-0000-0000-000000000002",
            "settings": {}
        }"#;
        assert!(serde_json::from_str::<ClientMessage>(json).is_err());
    }

    /// `infill_density` is a fraction (0.0–1.0) at the wire level — nothing
    /// is divided by 100 server-side. The previous percent-style protocol
    /// silently produced essentially-zero infill when the UI sent fractions.
    #[test]
    fn slicing_params_infill_density_is_a_fraction() {
        let json = r#"{
            "type": "Slice",
            "request_uuid": "00000000-0000-0000-0000-000000000003",
            "scene": [{ "file_id": "00000000-0000-0000-0000-000000000020" }],
            "settings": { "infill_density": 0.3 }
        }"#;
        let parsed: ClientMessage = serde_json::from_str(json).expect("parse");
        match parsed {
            ClientMessage::Slice { settings, .. } => {
                let settings = settings.expect("settings present");
                assert!((settings.infill_density - 0.3).abs() < 1e-9);
            }
            _ => panic!("expected Slice"),
        }
    }

    /// A `Slice` may carry a structured profile selection instead of the legacy
    /// flattened settings; it must round-trip and resolve.
    #[test]
    fn slice_message_with_profiles_round_trips() {
        let selection = crate::profiles::ProfileSelection {
            printer: crate::profiles::defaults::default_printer(),
            filament: crate::profiles::defaults::default_filament(),
            process: crate::profiles::defaults::default_process(),
            overrides: serde_json::json!({ "layer_height": 0.15 }),
        };
        let msg = serde_json::json!({
            "type": "Slice",
            "request_uuid": "00000000-0000-0000-0000-000000000004",
            "scene": [{ "file_id": "00000000-0000-0000-0000-000000000040" }],
            "profiles": selection,
        });
        let parsed: ClientMessage = serde_json::from_value(msg).expect("parse profiles slice");
        match parsed {
            ClientMessage::Slice {
                profiles, settings, ..
            } => {
                assert!(settings.is_none());
                let resolved = profiles
                    .expect("profiles present")
                    .resolve()
                    .expect("resolve");
                assert!((resolved.layer_height - 0.15).abs() < 1e-9);
            }
            _ => panic!("expected Slice"),
        }
    }
}
