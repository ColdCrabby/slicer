//! What a probe learned about a printer — and what it still needs to ask.
//!
//! These types are deliberately free of any transport: they are produced by the
//! pure derivation in [`super::klipper`], which compiles for wasm too, so the
//! browser build reaches the same conclusions as the server and the desktop
//! app instead of reimplementing them in TypeScript.
//!
//! The shape encodes one rule: **facts apply, preferences ask.** Anything the
//! machine's own config states outright lands in [`PrinterDetection::params`]
//! and needs no confirmation. Anything that is a choice — which start macro
//! convention to target, what an auxiliary fan is for — becomes a
//! [`DetectionQuestion`] with our best guess pre-selected, so the wizard can
//! always offer a finished profile and still let the user correct it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::profiles::printer::{BedShape, PrinterConnectionKind};

/// Everything we could learn about a printer by probing a single URL.
///
/// Every hardware field is optional: detection is best-effort and degrades
/// gracefully. A `reachable: false` result still carries a `message` explaining
/// why. When `kind` is identified but hardware fields are absent (OctoPrint /
/// PrusaLink), the wizard can still pre-select the transport and host.
///
/// Serializes to the same field shape as the WS `PrinterDetected` payload
/// (minus the `host` envelope) so the native Tauri `printer_detect` command and
/// the cloud WebSocket probe hand the UI an identical object.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct PrinterDetection {
    /// The host answered at least one probe.
    pub reachable: bool,
    /// Detected transport, or `None` when nothing answered.
    pub kind: PrinterConnectionKind,
    /// Human-readable summary (a success note or the failure reason).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Friendly name (Klipper hostname), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Model designation, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Manufacturer, when known.
    ///
    /// The *machine's* maker — never the firmware. Klipper runs on hundreds of
    /// different printers, so reporting it here would put "Klipper" in the
    /// profile's vendor field; [`Self::firmware`] is where that belongs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// G-code dialect the firmware speaks (`marlin`, `klipper`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware: Option<String>,
    /// Bed shape (rectangular / circular), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bed_shape: Option<BedShape>,
    /// Bed width / diameter (mm), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bed_width: Option<f64>,
    /// Bed depth (mm), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bed_depth: Option<f64>,
    /// Max Z height (mm), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bed_height: Option<f64>,
    /// True for delta / center-origin machines, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_at_center: Option<bool>,
    /// Nozzle diameter (mm), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nozzle_diameter_mm: Option<f64>,

    /// Sparse [`SlicingParams`] overlay read straight off the machine's config.
    ///
    /// Merged into the printer profile's `params` bag the same way a catalog
    /// preset's overrides are. A sparse bag rather than a field per reading:
    /// the alternative propagates every new value through this struct, the WS
    /// message, the TypeScript model and the wizard — four edits to learn one
    /// number.
    ///
    /// [`SlicingParams`]: crate::settings::params::SlicingParams
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub params: serde_json::Value,

    /// Plain-language account of each value in [`Self::params`] and where in
    /// the config it came from, so the wizard can show its work.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<DetectionFinding>,

    /// Setup decisions the config could not make for us.
    ///
    /// Every question carries a `suggested` option, and the wizard applies it
    /// up front — these refine a profile that is already complete, they never
    /// block one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<DetectionQuestion>,
}

/// One value we read from the printer, with its provenance.
///
/// Exists so the wizard can offer a "what we read from your printer" panel:
/// silently applying a dozen settings is only acceptable if the user can see
/// what was applied and check it against their own `printer.cfg`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DetectionFinding {
    /// Short label for the value, e.g. `Filament diameter`.
    pub label: String,
    /// The value, already formatted with its unit.
    pub value: String,
    /// Where it came from, as the config section reads, e.g. `[extruder]`.
    pub source: String,
}

impl DetectionFinding {
    /// Build a finding. `source` is written the way the section appears in
    /// `printer.cfg` so the user can search for it.
    pub fn new(
        label: impl Into<String>,
        value: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            source: source.into(),
        }
    }
}

/// A setup decision detection could not make on its own.
///
/// The engine supplies the machine-specific parts — which options exist, which
/// one the config points at, and the evidence for that lean. The headline
/// wording lives in the UI, keyed by [`Self::id`].
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DetectionQuestion {
    /// Stable id the UI keys its copy off: `machine_identity`,
    /// `macro_convention`, `bed_mesh`, `aux_fan`, `preferred_orientation`.
    pub id: String,
    /// Options, suggested one first.
    pub options: Vec<DetectionOption>,
    /// Id of the option detection recommends. Always set — a question with no
    /// safe default would stall the wizard, which is exactly what this design
    /// removes.
    pub suggested: String,
    /// What in the config produced the lean, shown as "why we think so".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    /// The config answered this outright — apply [`Self::suggested`] and do not
    /// ask.
    ///
    /// A decided question is still a question rather than a bare fact because
    /// its *effect* is UI-owned: which start-macro convention a host follows is
    /// something only the config knows, but the G-code that convention implies
    /// is template copy the engine has no business carrying. So the engine
    /// reports the conclusion and the wizard applies it, listing it among the
    /// things it read rather than the things it needs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub certain: bool,
}

/// One answer to a [`DetectionQuestion`], and the settings it implies.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DetectionOption {
    /// Stable within its question. Machine-specific ids (a fan object's name)
    /// carry their own [`Self::label`]; known ids fall back to the UI's copy.
    pub id: String,
    /// Label for options the UI cannot know in advance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Factual detail from the config, e.g. `[fan_generic rscs]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Sparse [`SlicingParams`] patch this option contributes.
    ///
    /// Empty for options whose effect is not a slicing parameter — a machine's
    /// vendor and model, or a preferred plate orientation, are profile fields
    /// rather than params, so the wizard applies those by option id. Anything
    /// that *is* a param rides here, where no UI needs to know about it.
    ///
    /// [`SlicingParams`]: crate::settings::params::SlicingParams
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub params: serde_json::Value,
}

impl DetectionOption {
    /// An option that changes nothing — the safe default for every question
    /// whose other answers write G-code.
    pub fn passive(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: None,
            detail: None,
            params: serde_json::Value::Null,
        }
    }

    /// An option carrying a sparse params patch.
    pub fn with_params(id: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            id: id.into(),
            label: None,
            detail: None,
            params,
        }
    }

    /// Attach a label for an option the UI cannot name in advance.
    pub fn labelled(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Attach the config detail this option was read from.
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}
