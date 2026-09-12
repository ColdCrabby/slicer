//! Slicing parameters: per-print and per-object settings.

use crate::gcode::GcodeFlavor;
use crate::infill::{InfillPattern, SurfacePattern};
pub use crate::mesh::transforms::MeshQuality;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Fan index constants for indexed M106 Pn commands.
pub mod fan_index {
    /// Part-cooling fan (default, P0).
    pub const PART_COOLING: u8 = 0;
    /// Hotend cooling fan (P1).
    pub const HOTEND: u8 = 1;
    /// Chamber fan (P2).
    pub const CHAMBER: u8 = 2;
    /// Auxiliary fan (P3).
    pub const AUX: u8 = 3;
}

/// Bounded override settings for auxiliary cooling fans (e.g. RSCS).
///
/// Aux fans operate in *hybrid* mode: a baseline speed is computed from the
/// normal adaptive cooling curve, then constrained triggers can temporarily
/// raise it for bridges or short layers.  Several safety bounds prevent
/// over-cooling sensitive materials or creating thermal shock from abrupt
/// speed changes.
///
/// # Computation order
/// ```text
/// 1. base       ← speed_for_layer_time(layer_time)
/// 2. boost      ← bridge_boost (if bridging) + short_layer_boost (if short layer)
/// 3. boosted    ← min(base + boost, boost_max_speed)
/// 4. scaled     ← boosted × speed_scale
/// 5. capped     ← min(scaled, max_speed_limit)
/// 6. rate-limited ← clamp(capped, prev − max_speed_change_per_layer, prev + …)
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct AuxFanOverrides {
    /// Additional speed fraction added when printing bridge spans.
    ///
    /// Bridges cool fast and tend to sag — a short burst of extra airflow
    /// improves bridging quality.  **Example:** `0.40` = +40%.
    #[schemars(
        description = "Additional fan speed fraction (0.0–1.0) added when printing bridges."
    )]
    pub bridge_boost: f64,

    /// Additional speed fraction added when the layer time is ≤ `layer_time_fast_s`.
    ///
    /// Short layers accumulate heat quickly; a small extra boost prevents
    /// layer adhesion problems and warping.  **Example:** `0.20` = +20%.
    #[schemars(
        description = "Additional fan speed fraction (0.0–1.0) added on very short layers."
    )]
    pub short_layer_boost: f64,

    /// Speed cap applied after all boosts, before scaling.
    ///
    /// Prevents the combined boost from exceeding a sensible ceiling even if
    /// both bridge and short-layer triggers fire simultaneously.
    #[schemars(description = "Maximum fan speed fraction (0.0–1.0) after applying all boosts.")]
    pub boost_max_speed: f64,

    /// Multiplicative scale applied to the final boosted speed.
    ///
    /// Useful when an aux fan (RSCS, side-blast, etc.) has different airflow
    /// characteristics than a direct part-cooling fan.  **Example:** `0.8` = 80% of
    /// computed speed.
    #[schemars(description = "Multiplier applied to the final speed (e.g. 0.8 = 80%).")]
    pub speed_scale: f64,

    /// Hard maximum speed limit for material safety (0.0–1.0).
    ///
    /// Each filament has a safe maximum cooling rate; exceeding it can cause
    /// layer delamination or warping.  This cap is enforced *after* scaling.
    #[schemars(description = "Hard maximum speed fraction (0.0–1.0) for material safety.")]
    pub max_speed_limit: f64,

    /// Maximum allowed speed change between consecutive layers (0.0–1.0).
    ///
    /// Prevents thermal shock from jumping from near-zero to full speed in one
    /// layer.  **Example:** `0.15` = at most 15% change per layer.
    #[schemars(description = "Maximum speed change per layer (0.0–1.0) to prevent thermal shock.")]
    pub max_speed_change_per_layer: f64,
}

impl AuxFanOverrides {
    /// Sensible defaults for an RSCS-style auxiliary cooling fan.
    pub fn default_rscs() -> Self {
        Self {
            bridge_boost: 0.40,
            short_layer_boost: 0.20,
            boost_max_speed: 0.95,
            speed_scale: 1.0,
            max_speed_limit: 1.0,
            max_speed_change_per_layer: 0.15,
        }
    }
}

/// Configuration for a single fan in a multi-fan printer system.
///
/// Fan speed is automatically adapted to the estimated layer print time:
/// fast layers (small features) get maximum cooling; slow layers get minimum.
///
/// For auxiliary fans (RSCS and similar systems) set `aux_overrides` to enable
/// bounded boost triggers and rate limiting on top of the baseline curve.
///
/// # Layer-time thresholds
/// ```text
/// layer_time ≤ layer_time_fast_s  →  speed = max_speed
/// layer_time ≥ layer_time_slow_s  →  speed = min_speed
/// between                         →  linear interpolation
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct FanConfig {
    /// Fan index for indexed M106 `Pn` commands.
    ///
    /// - `0` — part-cooling fan (default, `M106 S…`)
    /// - `1` — hotend fan
    /// - `2` — chamber fan
    /// - `3` — auxiliary fan
    #[schemars(
        description = "Fan index (P parameter in M106). 0=part-cooling, 1=hotend, 2=chamber, 3=aux."
    )]
    pub fan_index: u8,

    /// Custom Klipper fan object name.
    ///
    /// When set, overrides the default name derived from `fan_index` in the
    /// Klipper dialect (`fan`, `fan_hotend`, `fan_chamber`, `fan_aux`).
    /// Use this to map a fan index to a printer-specific Klipper fan object,
    /// e.g. `"rscs"`, `"side_blast"`, or any `[fan]`/`[fan_generic]` name
    /// defined in your `printer.cfg`.
    ///
    /// Ignored by Marlin/RepRap firmware — those use `fan_index` exclusively.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Optional Klipper fan object name override (e.g. 'rscs', 'side_blast'). Overrides the default name derived from fan_index."
    )]
    pub klipper_name: Option<String>,

    /// Minimum fan speed as a fraction `0.0`–`1.0`.
    ///
    /// Applied when the layer takes longer than `layer_time_slow_s` to print.
    #[schemars(description = "Minimum fan speed fraction (0.0–1.0) for slow/large layers.")]
    pub min_speed: f64,

    /// Maximum fan speed as a fraction `0.0`–`1.0`.
    ///
    /// Applied when the layer takes less than `layer_time_fast_s` to print.
    #[schemars(description = "Maximum fan speed fraction (0.0–1.0) for fast/small layers.")]
    pub max_speed: f64,

    /// Layer time threshold in seconds below which maximum fan speed is used.
    #[schemars(description = "Layer time (seconds) at or below which max_speed is used.")]
    pub layer_time_fast_s: f64,

    /// Layer time threshold in seconds above which minimum fan speed is used.
    #[schemars(description = "Layer time (seconds) at or above which min_speed is used.")]
    pub layer_time_slow_s: f64,

    /// Optional auxiliary fan bounded overrides (RSCS-style hybrid cooling).
    ///
    /// When `Some`, this fan operates in hybrid mode: the baseline adaptive
    /// speed is computed normally, then bridge/short-layer boosts and safety
    /// caps are applied on top.  `None` means pure adaptive cooling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Optional auxiliary fan boost and safety overrides. Enables RSCS-style hybrid cooling with bridge boost, short-layer boost, speed scaling, material safety cap, and rate limiting."
    )]
    pub aux_overrides: Option<AuxFanOverrides>,
}

impl FanConfig {
    /// Default configuration for a single part-cooling fan (P0).
    ///
    /// Uses 35% as the minimum speed — below this threshold most centrifugal
    /// part-cooling fans stall or become ineffective.  Maximum is 100%.
    /// Layer times are set to 10 s (fast) and 30 s (slow), matching
    /// typical OrcaSlicer defaults.
    pub fn default_part_cooling() -> Self {
        Self {
            fan_index: fan_index::PART_COOLING,
            klipper_name: None,
            min_speed: 0.35,
            max_speed: 1.0,
            layer_time_fast_s: 10.0,
            layer_time_slow_s: 30.0,
            aux_overrides: None,
        }
    }

    /// Compute the baseline fan speed fraction for the given layer time in seconds.
    ///
    /// Returns `max_speed` for short layers, `min_speed` for long layers,
    /// and a linearly interpolated value in between.
    ///
    /// When `layer_time_fast_s >= layer_time_slow_s` (degenerate configuration),
    /// returns `max_speed` for all layer times.
    ///
    /// This is the *baseline* speed before any [`AuxFanOverrides`] are applied.
    /// Use [`FanConfig::compute_speed`] to get the final speed including boosts,
    /// scaling, safety caps, and rate limiting.
    pub fn speed_for_layer_time(&self, layer_time_s: f64) -> f64 {
        if layer_time_s <= self.layer_time_fast_s
            || self.layer_time_fast_s >= self.layer_time_slow_s
        {
            self.max_speed
        } else if layer_time_s >= self.layer_time_slow_s {
            self.min_speed
        } else {
            let t = (layer_time_s - self.layer_time_fast_s)
                / (self.layer_time_slow_s - self.layer_time_fast_s);
            self.max_speed + t * (self.min_speed - self.max_speed)
        }
    }

    /// Compute the final fan speed for emission, applying any [`AuxFanOverrides`].
    ///
    /// # Arguments
    /// * `layer_time_s` — estimated layer print time in seconds
    /// * `has_bridges` — `true` if the current layer contains bridge paths
    /// * `prev_speed` — the speed emitted on the previous layer (for rate limiting)
    ///
    /// When `aux_overrides` is `None`, returns `speed_for_layer_time(layer_time_s)`
    /// unchanged.
    pub fn compute_speed(
        &self,
        layer_time_s: f64,
        has_bridges: bool,
        prev_speed: Option<f64>,
    ) -> f64 {
        let base = self.speed_for_layer_time(layer_time_s);

        let Some(aux) = &self.aux_overrides else {
            return base;
        };

        // 1. Accumulate boost from triggers
        let is_short_layer = layer_time_s <= self.layer_time_fast_s;
        let mut boost = 0.0_f64;
        if has_bridges {
            boost += aux.bridge_boost;
        }
        if is_short_layer {
            boost += aux.short_layer_boost;
        }

        // 2. Apply boost, capped at boost_max_speed
        let boosted = if boost > 0.0 {
            (base + boost).min(aux.boost_max_speed)
        } else {
            base
        };

        // 3. Apply speed scale
        let scaled = (boosted * aux.speed_scale).clamp(0.0, 1.0);

        // 4. Apply hard material safety cap
        let capped = scaled.min(aux.max_speed_limit);

        // 5. Apply rate limiting
        let rate_limited = if let Some(prev) = prev_speed {
            let delta = aux.max_speed_change_per_layer;
            capped.clamp(prev - delta, prev + delta)
        } else {
            capped
        };

        rate_limited.clamp(0.0, 1.0)
    }
}

/// Where to place the start/end point ("seam") of each closed perimeter loop.
///
/// Closed loops are cyclic and may begin at any vertex; the chosen vertex
/// becomes the visible blob/seam where extrusion starts and ends.  Different
/// policies trade off visual quality against travel distance.
///
/// Mirrors the seam options offered by PrusaSlicer / OrcaSlicer / Bambu Studio
/// so users can transfer their preferences.
///
/// `Nearest` minimises travel but scatters seams; `Rear` and `Aligned` stack
/// them into one line (aligned holds that line through a twisting model, since
/// it measures against a fixed XY direction rather than the loop's bounding
/// box); `SharpestCorner` falls back to `Nearest` on a cornerless loop; and
/// `Random` is deterministic per loop, seeded from the path geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SeamPosition {
    /// Whichever vertex the nozzle is nearest — quickest, but seams scatter.
    #[default]
    Nearest,
    /// At the back of the plate, where a display piece is least often looked at.
    Rear,
    /// Like rear, but holds its line even where the model twists.
    Aligned,
    /// Tucked into the sharpest corner, where a blob is expected and least seen.
    SharpestCorner,
    /// A different vertex every loop, so no seam line forms at all.
    Random,
}

impl SeamPosition {
    /// Parse a policy name from a CLI argument or config string
    /// (case-insensitive, hyphens and underscores both accepted).
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().replace('-', "_").as_str() {
            "nearest" => Some(Self::Nearest),
            "rear" => Some(Self::Rear),
            "aligned" => Some(Self::Aligned),
            "sharpest_corner" | "sharp_corner" | "sharp" | "corner" => Some(Self::SharpestCorner),
            "random" => Some(Self::Random),
            _ => None,
        }
    }

    /// Canonical name for emitting back into config / G-code comments.
    pub fn name(self) -> &'static str {
        match self {
            Self::Nearest => "nearest",
            Self::Rear => "rear",
            Self::Aligned => "aligned",
            Self::SharpestCorner => "sharpest_corner",
            Self::Random => "random",
        }
    }
}

/// Which wall (perimeter) generation algorithm to use.
///
/// Mirrors the "Wall generator" choice offered by PrusaSlicer / OrcaSlicer /
/// Bambu Studio: a robust classic offset generator, or the Arachne
/// variable-width generator.
///
/// `Classic` is deterministic and dependency-free: `wall_count` constant-width
/// beads per shell, plus one variable-width residual bead in whatever narrow
/// space remains. `Arachne` adapts the loop count to the local wall thickness
/// and follows the medial axis into thin features a fixed-width perimeter
/// cannot reach, after Kuipers et al. (2020). Arachne is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WallGenerator {
    /// Every wall the same width; anything too thin for a bead becomes gap fill.
    Classic,
    /// Walls that vary in width to reach thin features a fixed bead cannot.
    #[default]
    Arachne,
}

impl WallGenerator {
    /// Parse a generator name from a CLI argument or config string
    /// (case-insensitive, hyphens and underscores both accepted).
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().replace('-', "_").as_str() {
            "classic" | "offset" | "offsets" => Some(Self::Classic),
            "arachne" | "vwe" => Some(Self::Arachne),
            _ => None,
        }
    }

    /// Canonical name for emitting back into config / G-code comments.
    pub fn name(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Arachne => "arachne",
        }
    }
}

/// Support-structure style.
///
/// Both variants are generated by [`crate::core::generate_supports`].  `Normal`
/// projects each overhang straight down as a grid/zig-zag column; `Tree` tapers
/// and merges those columns into organic branches (a simplified approximation of
/// full branching tree support).  `support_threshold_angle`, `support_density`,
/// interface layers, and XY/Z clearance all apply to both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SupportType {
    /// Straight grid columns under every overhang — predictable, more to cut off.
    #[default]
    Normal,
    /// Branches that taper and merge, touching the model in fewer places.
    Tree,
}

/// First-layer bed-adhesion helper.
///
/// Drives skirt / brim / raft generation in [`crate::adhesion`].
///
/// The enum default is [`None`](Self::None): a bare
/// [`SlicingParams::default()`](crate::settings::params::SlicingParams) slice
/// produces only the object, with no adhesion geometry. Product intent (a skirt
/// on the standard PLA profile) is expressed by the process profiles in
/// [`crate::profiles::defaults`], which set `adhesion_type` explicitly, not by
/// this struct default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AdhesionType {
    /// The object alone, printed straight onto the plate.
    #[default]
    None,
    /// A loop traced around the object without touching it; primes the nozzle.
    Skirt,
    /// A flat apron fused to the object's first layer for extra bed grip.
    Brim,
    /// A full sacrificial base printed under the object.
    Raft,
}

/// Where brim material is placed relative to the object footprint.
///
/// Consumed by [`crate::adhesion`] when `adhesion_type = brim`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BrimType {
    /// Loops around the outer contour of every island (the common case).
    #[default]
    OuterOnly,
    /// Inside holes and concavities only, leaving the outer edge clean.
    InnerOnly,
    /// Both outer contours and holes get brim loops.
    OuterAndInner,
    /// Small discs at sharp corners only — least material where warping starts.
    Ears,
}

/// Which solid surfaces the ironing pass sweeps.
///
/// Mirrors the PrusaSlicer (`top`/`topmost`/`solid`) and OrcaSlicer
/// (`top surfaces`/`topmost surface only`/`all solid surfaces`) enumerations so
/// an imported profile maps onto one of these without loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IroningType {
    /// Every exposed top surface, on any layer (the common case).
    #[default]
    TopSurfaces,
    /// Only the model's highest face — the one a viewer actually looks down on.
    TopmostOnly,
    /// Every solid surface, internal floors included. Rarely worth the time.
    AllSolid,
}

/// How a plate holding several objects is printed.
///
/// The developer-facing rationale (how each order flows through the slicing
/// pipeline and the G-code generator) lives in the object-identity section of
/// AGENTS.md; the doc text here stays user-facing because it becomes the
/// setting's on-screen description.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrintSequence {
    /// Print every object together, rising one layer at a time.
    #[default]
    ByLayer,
    /// Finish each object completely before starting the next.
    ByObject,
}

impl PrintSequence {
    /// Parse a sequence name from a CLI argument or config string
    /// (case-insensitive, hyphens and underscores both accepted).
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().replace('-', "_").as_str() {
            "by_layer" | "layer" => Some(Self::ByLayer),
            "by_object" | "object" | "sequential" => Some(Self::ByObject),
            _ => None,
        }
    }

    /// Canonical name for emitting back into config / G-code comments.
    pub fn name(self) -> &'static str {
        match self {
            Self::ByLayer => "by_layer",
            Self::ByObject => "by_object",
        }
    }
}

/// Bed mesh leveling directive emitted at print start.
///
/// Off by default: most printers already handle leveling inside a firmware
/// macro or a one-time manual calibration, and a slicer-driven mesh command
/// on top of that would either duplicate the work or race it. Turning this on
/// hands the mesh step to the slicer instead, so it survives across different
/// custom start scripts without being hand-copied into each one.
///
/// `LoadProfile` emits Klipper `BED_MESH_PROFILE LOAD=<name>` or Marlin/RepRap
/// `M420 S1`; `Calibrate` emits `BED_MESH_CALIBRATE` / `G29` and then loads the
/// result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BedMeshMode {
    /// Leveling is left entirely to the printer's own start macro or config.
    #[default]
    Off,
    /// Reuses a mesh probed earlier, so it costs no print time.
    LoadProfile,
    /// Probes the bed before every print: always current, a few minutes a job.
    Calibrate,
}

/// Camera angle used when the UI renders the embedded G-code thumbnail.
///
/// The thumbnail is produced from a fixed, repeatable viewpoint (not the
/// user's live camera) so every slice yields a comparable preview. Angles are
/// expressed in the scene's world Z-up frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThumbnailView {
    /// Three-quarter view from the front-right, slightly above.
    #[default]
    Isometric,
    /// Straight-on from the front (−Y), a hair above the horizon.
    Front,
    /// Straight-on from the back (+Y).
    Rear,
    /// From the model's left (−X).
    Left,
    /// From the model's right (+X).
    Right,
    /// Top-down plan view (+Z looking down).
    Top,
}

/// Colour scheme used when the UI renders the embedded G-code thumbnail.
///
/// Fixed per the setting — deliberately independent of the operating-system or
/// application theme so the embedded preview is deterministic. Model colouring
/// (filament colour vs. neutral grey) still follows the viewer's own toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThumbnailTheme {
    /// A solid pale backdrop is baked into the image.
    Light,
    /// A solid near-black backdrop is baked into the image.
    Dark,
    /// No backdrop — a PNG cutout the printer's own screen shows through.
    #[default]
    Transparent,
}

/// How the model is coloured in the embedded thumbnail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThumbnailColorMode {
    /// A neutral grey plastic look, whatever is actually loaded.
    Generic,
    /// Takes the colour from the spool you sliced with, so the preview matches
    /// the viewer (default).
    #[default]
    Filament,
    /// One fixed colour you pick yourself, whatever the spool says.
    Custom,
}

/// A mid-print interruption: a manual pause, a filament/color change, or
/// arbitrary custom G-code, fired at a chosen layer or Z height.
///
/// See [`crate::gcode::GcodeDialect::pause_gcode`] and
/// [`crate::gcode::GcodeDialect::color_change_gcode`] for the firmware
/// commands emitted per [`GcodeFlavor`](crate::gcode::GcodeFlavor).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PauseTrigger {
    /// Where the trigger fires.
    #[serde(flatten)]
    pub position: TriggerPosition,
    /// What happens when it fires.
    pub action: TriggerAction,
}

/// Where a [`PauseTrigger`] fires.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "position_type", rename_all = "snake_case")]
pub enum TriggerPosition {
    /// Fire immediately after the layer-change block for this 1-based layer
    /// number (matches the `{layer_num}` placeholder semantics used
    /// elsewhere in G-code generation).
    AtLayer {
        /// 1-based layer number.
        layer: u32,
    },
    /// Fire once, on the first layer whose model Z is at or above this
    /// value (mirrors "pause at height" semantics from other slicers).
    AtZ {
        /// Model Z height in mm.
        z: f64,
    },
}

/// What a [`PauseTrigger`] does when it fires.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerAction {
    /// Pause the print for manual intervention.
    Pause,
    /// Perform a filament/color change.
    ColorChange,
    /// Emit arbitrary custom G-code, supporting the same `{z}`/`{layer_num}`/
    /// `{height}` placeholders as `custom_layer_script`.
    Custom {
        /// Raw G-code, one or more lines.
        gcode: String,
    },
}

/// Parameters that control how a model is sliced and printed.
///
/// All dimensional values are in millimeters; speeds in mm/s;
/// temperatures in °C; infill density as a fraction 0.0–1.0.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default)]
pub struct SlicingParams {
    #[schemars(description = "Layer height in mm.

Smaller values produce finer detail but increase print time.
**Typical:** 0.05–0.35 mm.", extend("x-group" = "Layer", "x-unit" = "mm", "x-step" = 0.01))]
    pub layer_height: f64,

    #[schemars(description = "Wall (perimeter) generation algorithm.

This decides the shape of every wall on the print, so wall widths, speeds and
seam settings tuned against one generator do not transfer exactly to the other.

**Default:** `arachne`.", extend("x-group" = "Walls", "x-tier" = "advanced", "x-widget" = "cards"))]
    #[serde(default = "SlicingParams::default_wall_generator")]
    pub wall_generator: WallGenerator,

    #[schemars(description = "Number of perimeter (wall) beads per layer.

Arachne places up to this many concentric wall paths around each shell polygon.
The innermost bead may have variable width when narrow space remains.
**Typical:** 2–4.", extend("x-group" = "Walls", "x-unit" = "count"))]
    #[serde(default = "SlicingParams::default_wall_count")]
    pub wall_count: usize,

    #[schemars(
        description = "Minimum allowed bead width as a fraction of nozzle diameter.

Beads narrower than `wall_line_width_min × nozzle_diameter_mm` are skipped entirely.
**Range:** 0.5–1.0.",
        extend("x-group" = "Walls", "x-tier" = "expert", "x-unit" = "ratio")
    )]
    #[serde(default = "SlicingParams::default_wall_line_width_min")]
    pub wall_line_width_min: f64,

    #[schemars(
        description = "Maximum allowed bead width as a fraction of nozzle diameter.

Variable-width beads are capped at this multiple to avoid excessive over-extrusion at corners.
**Range:** 1.0–2.0.",
        extend("x-group" = "Walls", "x-tier" = "expert", "x-unit" = "ratio")
    )]
    #[serde(default = "SlicingParams::default_wall_line_width_max")]
    pub wall_line_width_max: f64,

    #[schemars(
        description = "Minimum wall space (fraction of nozzle diameter) before bead count decreases.

When remaining space is narrower than `wall_transition_threshold × nozzle_diameter_mm`,
the algorithm widens the existing innermost bead instead of adding a new one.
**Typical:** 0.4–0.8.",
        extend("x-group" = "Walls", "x-tier" = "expert", "x-unit" = "ratio")
    )]
    #[serde(default = "SlicingParams::default_wall_transition_threshold")]
    pub wall_transition_threshold: f64,

    #[schemars(
        description = "Length (mm) over which a bead-count transition is smoothed.

Larger values produce a gradual width ramp at transitions; smaller values create abrupt changes.
**Typical:** 0.5–2.0 mm.",
        extend("x-group" = "Walls", "x-unit" = "mm", "x-step" = 0.05, "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_wall_transition_length")]
    pub wall_transition_length: f64,

    #[schemars(description = "Number of inner wall beads that absorb width variation.

When space is too narrow for a separate bead, up to this many innermost beads
are widened proportionally to fill the gap.

Classic generator only — Arachne varies bead width along the medial axis instead.
**Typical:** 1–2.", extend("x-group" = "Walls", "x-unit" = "count", "x-tier" = "expert", "x-relevant-when" = serde_json::json!({"field": "wall_generator", "equals": "classic"})))]
    #[serde(default = "SlicingParams::default_wall_distribution_count")]
    pub wall_distribution_count: usize,

    #[schemars(description = "Detect thin walls and print them as a single centered bead.

A **thin feature** is model material too narrow for even one full perimeter —
engraved text, a tapering rib, the card-slot fins of a card holder. When on, such
a feature is traced by a single variable-width bead; when off it is **not printed
at all** (the feature disappears from the part).

- `true` (default) — thin features are printed.
- `false` — thin features are skipped.

Classic generator only. Arachne fills thin features from the medial axis by
construction — that is what the generator is for — so it always prints them and
ignores this option.

Mirrors `thin_walls` (PrusaSlicer/Slic3r) / `detect_thin_wall` (OrcaSlicer), both
of which are likewise classic-only.", extend("x-group" = "Walls", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "wall_generator", "equals": "classic"})))]
    #[serde(default = "SlicingParams::default_thin_walls")]
    pub thin_walls: bool,

    #[schemars(
        description = "Minimum length in mm for a gap-fill bead to be kept.

Gap-fill beads shorter than this are dropped to avoid stringy sub-millimetre
dribbles the medial pass finds along faceted boundaries — the isolated \"splat\"
beads that waste print time on a retract/travel/un-retract cycle and risk
filament grinding for a mechanically-insignificant dab.  Set to `0` to use the
automatic default (twice the nozzle diameter), which matches the faceting-noise
floor used when de-noising the medial skeleton; the residual such short beads
would have filled is bridged by the squish of the flanking wall beads.

Arachne generator only — the classic generator emits a single residual bead per
shell rather than walking a medial skeleton.
**Typical:** 0.4–1.0 mm.",
        extend("x-group" = "Walls", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "expert", "x-relevant-when" = serde_json::json!({"field": "wall_generator", "equals": "arachne"}))
    )]
    #[serde(default = "SlicingParams::default_gap_fill_min_length_mm")]
    pub gap_fill_min_length_mm: f64,

    #[schemars(
        description = "Minimum medial-axis angle (degrees) at which a bead-count transition may occur.

Arachne-walk only.  A bead is added or removed along the medial axis only where
the local branch angle exceeds this, so ribs are not placed on near-parallel
edges where the count is ambiguous.
**Typical:** 5–20°.",
        extend("x-group" = "Walls", "x-unit" = "deg", "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_wall_transition_angle")]
    pub wall_transition_angle: f64,

    #[schemars(
        description = "Minimum spacing (mm) between adjacent bead-count transitions.

Arachne-walk only.  Transitions closer together than this are merged, de-noising
rapid count flip-flops along a faceted or slightly-tapering wall.
**Typical:** 0.05–0.3 mm.",
        extend("x-group" = "Walls", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_wall_transition_filter_distance")]
    pub wall_transition_filter_distance: f64,

    #[schemars(description = "Where to place the seam (start/end point) of each closed perimeter loop.

Supported values:
- `nearest` — closest vertex to current nozzle position (fastest, scattered seams).
- `rear` — vertex with maximum Y (single seam line on the back of the model).
- `aligned` — vertex closest to a fixed direction; consistent across layers.
- `sharpest_corner` — vertex with the sharpest convex angle (hidden in geometry).
- `random` — different random vertex per loop (no visible seam line).

**Default:** `aligned`.", extend("x-group" = "Walls", "x-tier" = "advanced"))]
    #[serde(default = "SlicingParams::default_seam_position")]
    pub seam_position: SeamPosition,

    #[schemars(description = "Print the outer (external) wall before the inner walls.

- `false` (default) — inner walls first, outer wall **last**.  The outer wall is
  laid against already-solid inner perimeters, giving the cleanest visible
  surface and crispest dimensions.  Matches the PrusaSlicer / OrcaSlicer / Cura
  default.
- `true` — outer wall first.  Can improve overhang adhesion (the external
  perimeter is anchored to the layer below before the inner walls push against
  it) at some cost to surface finish.

Mirrors `external_perimeters_first` (PrusaSlicer/Slic3r) / `wall_sequence`
(OrcaSlicer).", extend("x-group" = "Walls", "x-tier" = "advanced"))]
    #[serde(default = "SlicingParams::default_external_perimeters_first")]
    pub external_perimeters_first: bool,

    #[schemars(description = "Add extra perimeter loops where gaps would otherwise remain.

When a shell is locally thicker than `wall_count` beads but too thin for sparse
infill to fill cleanly, the leftover core is filled with additional concentric
perimeter loops instead of being left as a gap.  Only fires in narrow residual
regions (up to `extra_perimeters_max_gap × nozzle_diameter_mm` wide); wide cores
still get normal infill, so this never turns a part solid.

Mirrors `extra_perimeters` (PrusaSlicer/Slic3r).
**Default:** off.", extend("x-group" = "Walls", "x-tier" = "advanced"))]
    #[serde(default = "SlicingParams::default_extra_perimeters")]
    pub extra_perimeters: bool,

    #[schemars(
        description = "Widest residual core (as a multiple of nozzle diameter) that `extra_perimeters` will fill with loops.

A residual core wider than `extra_perimeters_max_gap × nozzle_diameter_mm` is
left for sparse infill; a narrower one is filled with extra concentric
perimeters.
**Typical:** 2–4.",
        extend("x-group" = "Walls", "x-unit" = "mm", "x-step" = 0.05, "x-tier" = "expert", "x-relevant-when" = serde_json::json!({"field": "extra_perimeters", "equals": true}))
    )]
    #[serde(default = "SlicingParams::default_extra_perimeters_max_gap")]
    pub extra_perimeters_max_gap: f64,

    #[schemars(description = "Ensure a minimum solid vertical-shell thickness on sloped surfaces.

On a near-vertical wall whose cross-section drifts layer over layer, the
perimeters of neighbouring layers may not overlap, leaving a thin spot in the
side wall.  When enabled, any interior region that is **not** backed by
perimeters in the layers immediately above *and* below is filled solid, so the
side wall keeps a continuous shell.

Mirrors `ensure_vertical_shell_thickness` (PrusaSlicer/Slic3r).
**Default:** off.", extend("x-group" = "Walls", "x-tier" = "advanced"))]
    #[serde(default = "SlicingParams::default_ensure_vertical_shell_thickness")]
    pub ensure_vertical_shell_thickness: bool,

    #[schemars(description = "Route travel moves to avoid crossing perimeter walls.

Instead of moving in a straight line to the next extrusion, the nozzle detours
around the inside of the current island's walls, so a travel move never drags
the (oozing) nozzle across a finished outer surface.  Reduces surface scarring
and stringing at the small cost of longer travels and slightly more planning
time.

Mirrors `avoid_crossing_perimeters` (PrusaSlicer/Slic3r) / `reduce_crossing_wall`
(OrcaSlicer).
**Default:** off.", extend("x-group" = "Walls", "x-tier" = "advanced"))]
    #[serde(default = "SlicingParams::default_avoid_crossing_perimeters")]
    pub avoid_crossing_perimeters: bool,

    #[schemars(description = "Spiral (vase) mode — print a single continuous outer wall whose Z \
ramps smoothly over each layer, producing a seamless single-wall vase with no Z-seam.

When enabled the slicer forces a single perimeter and turns off everything that would break the \
continuous spiral: sparse infill, top surfaces, retraction and Z-hop are all disabled. The solid \
bottom layers (`bottom_layers`) are kept as the base — set `bottom_layers` to `0` for an \
open-bottomed tube. Best on **solid, single-island** models; only the outermost contour of each \
layer is spiralized (interior holes are ignored). Layers with more than one island fall back to \
normal (non-spiral) printing with a warning.

**Default:** `false`.", extend("x-group" = "Walls", "x-tier" = "advanced"))]
    #[serde(default = "SlicingParams::default_spiral_vase")]
    pub spiral_vase: bool,

    #[schemars(description = "Perturb the outer wall with a rough, hand-textured finish.

Every vertex of the outer wall is displaced perpendicular to the wall by a
small random amount, replacing the smooth outer surface with a fuzzy,
non-uniform texture. Purely cosmetic — inner walls, infill and every other
pass are unaffected.

Mirrors `fuzzy_skin` (PrusaSlicer/Slic3r); this engine fuzzes only the outer
wall, matching Prusa's `external` mode.
**Default:** off.", extend("x-group" = "Walls", "x-tier" = "advanced"))]
    #[serde(default = "SlicingParams::default_fuzzy_skin")]
    pub fuzzy_skin: bool,

    #[schemars(
        description = "Maximum perpendicular displacement (mm) `fuzzy_skin` applies to each point.

Mirrors `fuzzy_skin_thickness` (PrusaSlicer/Slic3r).
**Default:** 0.15 mm. **Typical:** 0.1–0.5 mm.",
        extend("x-group" = "Walls", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "fuzzy_skin", "equals": true}))
    )]
    #[serde(default = "SlicingParams::default_fuzzy_skin_thickness_mm")]
    pub fuzzy_skin_thickness_mm: f64,

    #[schemars(
        description = "Spacing (mm) between the points `fuzzy_skin` perturbs along the outer wall.

Smaller values pack in more, finer bumps; larger values give a coarser
texture with fewer, wider ones.
Mirrors `fuzzy_skin_point_dist` (PrusaSlicer/Slic3r).
**Default:** 0.5 mm. **Typical:** 0.3–1.0 mm.",
        extend("x-group" = "Walls", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "fuzzy_skin", "equals": true}))
    )]
    #[serde(default = "SlicingParams::default_fuzzy_skin_point_dist_mm")]
    pub fuzzy_skin_point_dist_mm: f64,

    #[schemars(description = "Infill density as a fraction (0.0–1.0).

- `0.0` = completely hollow
- `0.15`–`0.3` = typical range for good strength/speed balance
- `1.0` = fully solid", extend("x-group" = "Infill", "x-unit" = "fraction"))]
    pub infill_density: f64,

    #[schemars(description = "Infill pattern geometry.

Supported values:
- `rectilinear` — alternating straight lines (fastest)
- `aligned-rectilinear` — straight lines that keep the same angle on every layer
- `grid` — crossed lines forming a grid
- `triangles` — three line sets 60° apart
- `tri-hexagon` — triangles with every third set offset, forming stars
- `cubic` — three line sets whose phase walks with height, forming stacked cubes
- `honeycomb` — hexagonal cells (good strength-to-weight ratio)
- `concentric` — loops following the outline
- `gyroid` — smooth triply-periodic surface (excellent isotropy)
- `tpms-d` — triply-periodic minimal surface, diamond variant

Every pattern deposits the density you ask for: a pattern that draws several
line sets across the same area splits the density between them.", extend("x-group" = "Infill"))]
    #[serde(default = "SlicingParams::default_infill_pattern")]
    pub infill_pattern: InfillPattern,

    #[schemars(description = "Base angle in degrees for sparse infill lines.

Alternating layers rotate by +90° on top of this base angle to create a crossing pattern.
**Default:** 45°.", extend("x-group" = "Infill", "x-unit" = "deg", "x-tier" = "advanced"))]
    #[serde(default = "SlicingParams::default_infill_base_angle")]
    pub infill_base_angle: f64,

    #[schemars(
        description = "How far a sparse-infill line may run along the inner wall to anchor itself, \
as a percentage of the infill line spacing.

Every sparse-infill line ends in mid-air against the wall.  Letting it turn and
follow the wall for a short distance welds it to the perimeter, so the infill
actually braces the shell instead of just touching it — and two lines that meet
around a short stretch of wall can be joined into one continuous move, removing
a retract/travel pair.

- `400` — OrcaSlicer's default; a good balance of bonding and travel savings.
- `0` — never extend a lone line end along the wall (lines may still be joined
  in pairs when the wall between them is shorter than `infill_anchor_max_mm`).

Has no effect when `infill_anchor_max_mm` is `0`.
**Typical:** 0–1000 %.",
        extend("x-group" = "Infill", "x-tier" = "expert", "x-unit" = "percent")
    )]
    #[serde(default = "SlicingParams::default_infill_anchor_percent")]
    pub infill_anchor_percent: f64,

    #[schemars(
        description = "Longest stretch of inner wall, in mm, that may be used to join two \
sparse-infill lines into one continuous path.

When the wall between the end of one infill line and the start of the next is
shorter than this, the two are merged and the wall segment is printed as part of
the infill — one move instead of two plus a travel.

- `20` — OrcaSlicer's default.
- `0` — turns anchoring off completely; every infill line is printed on its own.

**Typical:** 0–50 mm.",
        extend("x-group" = "Infill", "x-unit" = "mm", "x-step" = 1, "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_infill_anchor_max_mm")]
    pub infill_anchor_max_mm: f64,

    #[schemars(
        description = "Print sparse infill only every N layers, at N× the height.

Sparse infill does not need to be as finely layered as the walls.  Combining it
saves a lot of print time: the walls still print every layer, but the infill is
skipped until the top layer of each group, where it is extruded thicker to make
up for the layers it stood in for.

Only infill that exists on *every* layer of a group is combined, so solid
surfaces, bridges and changing cross-sections are never affected.  The combined
height is capped by `infill_combination_max_layer_height_mm` (and never exceeds
the nozzle diameter) — going beyond that would ask the nozzle to lay a bead
taller than its own orifice.

**Default:** `1` (no combining).",
        extend("x-group" = "Infill", "x-unit" = "count", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_infill_every_layers")]
    pub infill_every_layers: u32,

    #[schemars(
        description = "Tallest combined sparse-infill layer in mm, used with `infill_every_layers`.

Set to `0` to use the nozzle diameter, which is the practical ceiling — a bead
cannot reliably be laid taller than the orifice that extrudes it.  A smaller
value combines fewer layers per group.
**Default:** `0` (use the nozzle diameter).",
        extend("x-group" = "Infill", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_infill_combination_max_layer_height_mm")]
    pub infill_combination_max_layer_height_mm: f64,

    #[schemars(
        description = "Force a fully solid layer inside the part every N layers.

Adds internal solid layers that a normal shell calculation would not produce —
they act like hidden floors bracing the sparse infill, which stiffens tall
hollow parts and gives the layers above a dense base to print on.

`0` disables it.  Set it very high (e.g. `9999`) and only the layers a solid
sheet still fits under will be filled.
**Default:** `0` (off).",
        extend("x-group" = "Infill", "x-unit" = "count", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_solid_infill_every_layers")]
    pub solid_infill_every_layers: u32,

    #[schemars(description = "Default print speed in mm/s used as a fallback.

Slower speeds improve layer adhesion and surface quality; faster speeds reduce print time.
Role-specific speeds (perimeter_speed, infill_speed, etc.) take precedence when set to a
positive value.
**Typical:** 40–100 mm/s.", extend("x-group" = "Speed", "x-unit" = "mm_s"))]
    pub print_speed: f64,

    #[schemars(
        description = "Speed for outer and inner perimeter (wall) extrusions in mm/s.

Lower speeds improve surface quality and layer adhesion on perimeters.
Set to `0` to fall back to `print_speed`.
**Typical:** 40–50 mm/s.",
        extend("x-group" = "Speed", "x-unit" = "mm_s", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_perimeter_speed")]
    pub perimeter_speed: f64,

    #[schemars(
        description = "Speed for sparse infill extrusions in mm/s.

Higher speeds are acceptable for infill since it is not visible.
Set to `0` to fall back to `print_speed`.
**Typical:** 60–80 mm/s.",
        extend("x-group" = "Speed", "x-unit" = "mm_s", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_infill_speed")]
    pub infill_speed: f64,

    #[schemars(
        description = "Speed for bridge extrusions spanning unsupported gaps in mm/s.

Bridges print in mid-air, so the strand is not squished onto a layer below like
every other extrusion.  Crawling across the gap gives full part-cooling
(`bridge_fan_speed`) time to freeze the strand in place before it can sag.
Set to `0` to fall back to `print_speed`.
**Typical:** 10–25 mm/s. **Default:** 10 mm/s.",
        extend("x-group" = "Speed", "x-unit" = "mm_s", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_bridge_speed")]
    pub bridge_speed: f64,

    #[schemars(
        description = "Enable dynamic overhang speed & cooling.

Classifies perimeter segments by *overhang degree* — how much of the extrusion
width hangs over unsupported air below it — so each degree can print at its own
speed (`overhang_1_4_speed`…`overhang_4_4_speed`) and with extra part-cooling
airflow (`overhang_fan_speed`).  Mirrors the OrcaSlicer / PrusaSlicer *slow down
for overhangs* feature.  Set to `false` to print every overhang wall at a single
`bridge_speed` instead.
**Default:** true.",
        extend("x-group" = "Speed", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_enable_overhang_speed")]
    pub enable_overhang_speed: bool,

    #[schemars(
        description = "Speed for lightly-overhanging perimeters (0–25% of the line unsupported), in mm/s.

`0` = print at the normal `perimeter_speed` (no slowdown).  This band still sits
almost entirely on the layer below, so the default leaves it at full speed —
slowing it taxes a large share of ordinary walls on curved models for no gain.
**Default:** 0 (no slowdown).",
        extend("x-group" = "Speed", "x-unit" = "mm_s", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_overhang_degree_speed")]
    pub overhang_1_4_speed: f64,

    #[schemars(
        description = "Speed for moderately-overhanging perimeters (25–50% unsupported), in mm/s.

`0` = print at the normal `perimeter_speed` (no slowdown).  Half of this bead
still rests on the layer below, so the default leaves it at full speed.
**Default:** 0 (no slowdown).",
        extend("x-group" = "Speed", "x-unit" = "mm_s", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_overhang_degree_speed")]
    pub overhang_2_4_speed: f64,

    #[schemars(
        description = "Speed for steep overhanging perimeters (50–75% unsupported), in mm/s.

`0` = inherit `bridge_speed`, so this band tracks that setting instead of
pinning a second number that can drift out of sync with it.
**Typical:** 20–35 mm/s.",
        extend("x-group" = "Speed", "x-unit" = "mm_s", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_overhang_degree_speed")]
    pub overhang_3_4_speed: f64,

    #[schemars(
        description = "Speed for near-fully-unsupported perimeters (75–100% unsupported), in mm/s.

The steepest, most sag-prone band — effectively extruding into air, but without
a bridge's anchored far end to tension against, so it wants to run slower than
`bridge_speed`.  `0` = inherit `bridge_speed`.
**Default:** 8 mm/s.",
        extend("x-group" = "Speed", "x-unit" = "mm_s", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_overhang_4_4_speed")]
    pub overhang_4_4_speed: f64,

    #[schemars(
        description = "Slow down perimeters that are likely to curl upward.

Clamps both steep overhang degrees (50–100% unsupported) to the slowest
configured overhang speed, so a curling wall never outruns the most conservative
setting.  Off by default because it collapses the 50–75% band into the 75–100%
one, discarding the grading — enable it deliberately when a model curls.
**Default:** false.",
        extend("x-group" = "Speed", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_slowdown_for_curled_perimeters")]
    pub slowdown_for_curled_perimeters: bool,

    #[schemars(
        description = "Flow ratio for bridge extrusions (0.0–1.5).

Bridge lines are laid one nozzle-diameter apart, so a value **above 1.0** widens
each bead until neighbouring strands overlap and fuse into a continuous, smooth
floor — the opposite of the old under-extrude approach that left thin, gappy,
sag-prone strands.  Pair it with a slow `bridge_speed` and full `bridge_fan_speed`
so the fatter bead freezes in place before it sags.
**Default:** 1.5 (150% of normal flow).",
        extend("x-group" = "Quality", "x-tier" = "advanced", "x-unit" = "ratio")
    )]
    #[serde(default = "SlicingParams::default_bridge_flow_ratio")]
    pub bridge_flow_ratio: f64,

    #[schemars(
        description = "Minimum area in mm² for an unsupported region to be classified as a bridge.

Tiny slivers of unsupported area (caused by sub-millimetre layer-to-layer wall
jitter on features like embossed text or fine ridges) below this threshold are
classified as ordinary `BottomSurface` solid infill instead of `Bridge`.  This
matches OrcaSlicer's `min_bridge_area` filter and prevents stippling of the
preview with one-line bridge fragments.
**Default:** 0.5 mm² (≈ a 0.7 × 0.7 mm sliver). Set to `0.0` to disable.",
        extend("x-group" = "Quality", "x-unit" = "mm2", "x-step" = 0.01, "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_bridge_min_area_mm2")]
    pub bridge_min_area_mm2: f64,

    #[schemars(
        description = "Morphological-opening radius in mm applied to bridge regions.

The unsupported area is eroded inward by this amount and then dilated back —
removing thin slivers and filament-thin connecting strands that arise from
sub-pixel layer-to-layer geometry differences.  Cleans up the noisy bridges
that show up around fine surface features (e.g. Benchy's embossed name).
**Default:** 0.05 mm (just enough to wipe sub-pixel rounding from
Clipper2's Centi quantisation; small enough to preserve real
0.4 mm-wide bridge frames around windows).  Set to `0.0` to disable.",
        extend("x-group" = "Quality", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_bridge_noise_filter_mm")]
    pub bridge_noise_filter_mm: f64,

    #[schemars(
        description = "Anchor expansion length in mm at each end of every bridge.

After detecting the unsupported region, it is dilated by this amount and
re-clipped to the layer footprint so each bridge strand bites into the
adjacent supported solid material.  Without this the bridge ends mid-air at
the wall edge instead of being anchored, causing droopy strands.  Matches
PrusaSlicer / OrcaSlicer `bridge_anchor` behaviour.
**Default:** 0.5 mm.  Larger values cause bridges around small features
(text, embossed details) to expand past the first inner wall and look
visually wrong, even though they print fine.  Set to `0.0` to disable
anchoring.",
        extend("x-group" = "Quality", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_bridge_anchor_mm")]
    pub bridge_anchor_mm: f64,

    #[schemars(
        description = "Bridging angle override in degrees (0–180).

Leave at `0` to detect the direction automatically: the slicer picks the axis
that makes every strand span the *short* dimension of the gap, which is what
keeps a bridge from sagging.  Any other value is used for **every** bridge on
the model — useful when a part's bridges all run one way and the automatic
choice flip-flops between layers.

Following PrusaSlicer/OrcaSlicer, `0` is the auto trigger, so use **180** to
force a horizontal (0°) bridge direction.
**Default:** `0` (automatic).",
        extend("x-group" = "Quality", "x-unit" = "deg", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_bridge_angle")]
    pub bridge_angle: f64,

    #[schemars(
        description = "Speed for top and bottom solid surface infill in mm/s.

Slightly slower than infill to improve surface finish.
Set to `0` to fall back to `print_speed`.
**Typical:** 40–50 mm/s.",
        extend("x-group" = "Speed", "x-unit" = "mm_s", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_top_surface_speed")]
    pub top_surface_speed: f64,

    #[schemars(
        description = "Speed for gap-fill (thin-wall medial) extrusions in mm/s.

Gap fill lays narrow, variable-width beads into spaces too thin for a full
perimeter.  A slower speed keeps pressure stable in these short, narrow moves.
Set to `0` to fall back to `perimeter_speed` (then `print_speed`).
**Typical:** 20–40 mm/s.",
        extend("x-group" = "Speed", "x-unit" = "mm_s", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_gap_fill_speed")]
    pub gap_fill_speed: f64,

    #[schemars(
        description = "Speed for support material in mm/s.

Support is sacrificial and sparse, so it is usually run faster than the model — but it is also
poorly anchored, so going too fast knocks columns over. Set to `0` to fall back to `print_speed`.
**Typical:** 40–80 mm/s.",
        extend("x-group" = "Speed", "x-unit" = "mm_s", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_support_speed")]
    pub support_speed: f64,

    #[schemars(
        description = "Wall overlap flow compensation strength (0.0–1.0).

Where wall beads run closer than their combined width — tight slots, ~180°
hairpins, acute concave corners — a bead would deposit material into space an
adjacent bead already filled (over-extrusion, blobs).  This scales extrusion
*down* across the overlap so the total deposited volume stays correct.  `0.0`
disables it; `1.0` fully compensates.

The Arachne generators already place non-overlapping beads (overlap is removed in
the coverage/beading step and gap fill lives in the *uncovered* residual), so
compensation is unnecessary there and **off by default** — enabling it would
shed clean walls that abut gap fill at nominal spacing.  Raise it only for a
generator that emits genuinely overlapping walls.
**Default:** 0.0.",
        extend("x-group" = "Walls", "x-tier" = "expert", "x-unit" = "fraction")
    )]
    #[serde(default = "SlicingParams::default_wall_overlap_compensation")]
    pub wall_overlap_compensation: f64,

    #[schemars(
        description = "Speed for all extrusions on the first layer in mm/s.

Slower first-layer speeds improve bed adhesion.
Set to `0` to fall back to `print_speed`.
**Typical:** 20–30 mm/s.",
        extend("x-group" = "Speed", "x-unit" = "mm_s")
    )]
    #[serde(default = "SlicingParams::default_first_layer_speed")]
    pub first_layer_speed: f64,

    #[schemars(
        description = "Part-cooling fan speed for normal extrusions as a fraction (0.0–1.0).

- `0.0` = fan off
- `1.0` = full speed
**Typical:** 1.0 (100%).",
        extend("x-group" = "Cooling", "x-unit" = "fraction")
    )]
    #[serde(default = "SlicingParams::default_fan_speed")]
    pub fan_speed: f64,

    #[schemars(
        description = "Part-cooling fan speed when printing bridge extrusions as a fraction (0.0–1.0).

High fan speeds cool bridge material rapidly, reducing sag.
**Typical:** 1.0 (100%).",
        extend("x-group" = "Cooling", "x-tier" = "advanced", "x-unit" = "fraction")
    )]
    #[serde(default = "SlicingParams::default_bridge_fan_speed")]
    pub bridge_fan_speed: f64,

    #[schemars(
        description = "Part-cooling fan speed while printing overhang perimeters, as a fraction (0.0–1.0).

While the nozzle prints overhang segments above `overhang_fan_threshold`, the
part-cooling fan is raised to this speed and restored to the layer's normal
cooling afterwards, so sag-prone overhangs get a burst of extra airflow.  `0` =
never override the layer's normal fan speed.
**Default:** 1.0 (100%).",
        extend("x-group" = "Cooling", "x-tier" = "advanced", "x-unit" = "fraction")
    )]
    #[serde(default = "SlicingParams::default_overhang_fan_speed")]
    pub overhang_fan_speed: f64,

    #[schemars(
        description = "Overhang degree above which `overhang_fan_speed` engages, as an unsupported fraction (0.0–1.0).

The default `0.5` cools only the steep 50–100% overhangs, matching where the
overhang-perimeter classifier already kicks in.  Lowering it also cools the
milder degrees — at the cost of splitting otherwise-uniform walls into separate
fan regions.
**Default:** 0.5.",
        extend("x-group" = "Cooling", "x-tier" = "advanced", "x-unit" = "fraction")
    )]
    #[serde(default = "SlicingParams::default_overhang_fan_threshold")]
    pub overhang_fan_threshold: f64,

    #[schemars(
        description = "Part-cooling fan speed on the first layer as a fraction (0.0–1.0).

Typically disabled on the first layer to improve bed adhesion.
**Typical:** 0.0 (off).",
        extend("x-group" = "Cooling", "x-tier" = "advanced", "x-unit" = "fraction")
    )]
    #[serde(default = "SlicingParams::default_first_layer_fan_speed")]
    pub first_layer_fan_speed: f64,

    #[schemars(
        description = "Minimum time a layer (other than the first) is allowed to take, in seconds.

When a layer's estimated print time falls below this floor, its feedrates are
scaled down (never below `min_print_speed`) so the layer takes at least this
long — giving each layer time to cool before the next one lands. Any shortfall
still remaining once `min_print_speed` is reached is made up with a dwell.
`0` = disabled.
**Typical:** 5–15 s for small/detailed parts, `0` to disable.",
        extend("x-group" = "Cooling", "x-unit" = "s", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_min_layer_time_s")]
    pub min_layer_time_s: f64,

    #[schemars(
        description = "Slowest print speed the minimum-layer-time slowdown may drop to, in mm/s.

Once a layer's feedrates are scaled down to this floor, any remaining time
needed to reach `min_layer_time_s` is made up with a dwell instead of slowing
further — keeping extrusion fast enough to avoid heat-creep or grinding.
Ignored when `min_layer_time_s` is `0`.
**Typical:** 10 mm/s.",
        extend(
            "x-group" = "Cooling",
            "x-tier" = "advanced",
            "x-unit" = "mm_s",
            "x-relevant-when" = serde_json::json!({"field": "min_layer_time_s", "greaterThan": 0})
        )
    )]
    #[serde(default = "SlicingParams::default_min_print_speed")]
    pub min_print_speed: f64,

    #[schemars(
        description = "Coasting distance in mm: stop extruding this far before the end of a perimeter.

Reduces nozzle pressure at the seam, preventing blobs and improving surface quality.
Set to `0.0` to disable.
**Typical:** 0.1–0.3 mm.",
        extend("x-group" = "Speed", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_coasting_distance_mm")]
    pub coasting_distance_mm: f64,

    #[schemars(description = "Nozzle temperature in °C.

Material guidelines:
- **PLA:** 200–210 °C
- **PETG:** 230–250 °C
- **ABS:** 240–260 °C", extend("x-group" = "Temperature", "x-unit" = "celsius"))]
    pub nozzle_temp: f64,

    #[schemars(description = "Heated bed temperature in °C.

Material guidelines:
- **PLA:** 60–80 °C
- **PETG:** 80–100 °C
- **ABS:** 100–120 °C

Set to `0` for an unheated bed.", extend("x-group" = "Temperature", "x-unit" = "celsius"))]
    pub bed_temp: f64,

    #[schemars(
        description = "Number of solid top layers (horizontal surfaces facing up).

More layers improve surface quality and reduce infill show-through.
**Typical:** 4–6 layers at 0.2 mm layer height.",
        extend("x-group" = "Surfaces", "x-unit" = "count")
    )]
    #[serde(default = "SlicingParams::default_top_layers")]
    pub top_layers: usize,

    #[schemars(
        description = "Number of solid bottom layers (horizontal surfaces facing down).

More layers improve bottom surface finish and bed adhesion strength.
**Typical:** 3–4 layers.",
        extend("x-group" = "Surfaces", "x-unit" = "count")
    )]
    #[serde(default = "SlicingParams::default_bottom_layers")]
    pub bottom_layers: usize,

    #[schemars(
        description = "Angle in degrees for top/bottom solid surface infill lines.

Changing from the default can improve finish on curved or organic models.
**Default:** 45°.",
        extend("x-group" = "Surfaces", "x-unit" = "deg", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_surface_infill_angle")]
    pub surface_infill_angle: f64,

    #[schemars(
        description = "Fill pattern for the **top** solid surface.

Supported values:
- `monotonic-line` — parallel lines all drawn in the same direction, never
  connected (**default**, matching OrcaSlicer). The most uniform-looking top.
- `monotonic` — same one-way sweep, but consecutive line ends are joined along
  the surface boundary, so there is less travel.
- `rectilinear` — classic back-and-forth serpentine.
- `aligned-rectilinear` — serpentine that keeps the same angle on every layer
  instead of cross-hatching.
- `concentric` — loops following the surface outline.

\"Monotonic\" means every line is drawn in the same direction: the nozzle never
returns across a finished line, which is what removes the mottled, direction-
dependent sheen a serpentine leaves on a visible top surface.",
        extend("x-group" = "Surfaces", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_top_surface_pattern")]
    pub top_surface_pattern: SurfacePattern,

    #[schemars(
        description = "Fill pattern for the **bottom** solid surface.

Same choices as `top_surface_pattern`. **Default:** `monotonic` — the bottom is
against the bed, so the short boundary connectors cost nothing visually and save
travel.",
        extend("x-group" = "Surfaces", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_bottom_surface_pattern")]
    pub bottom_surface_pattern: SurfacePattern,

    #[schemars(
        description = "Fill pattern for **internal** solid infill.

Used for the dense layers `solid_infill_every_layers` inserts inside the part.
Same choices as `top_surface_pattern`. **Default:** `monotonic`.",
        extend("x-group" = "Surfaces", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_internal_solid_infill_pattern")]
    pub internal_solid_infill_pattern: SurfacePattern,

    #[schemars(description = "Filament diameter in mm.

Used to calculate extrusion volume from feed distance. Standard sizes:
- `1.75 mm` — most common
- `2.85 mm` — some older or larger-format printers", extend("x-group" = "Material", "x-unit" = "mm", "x-step" = 0.05))]
    #[serde(default = "SlicingParams::default_filament_diameter_mm")]
    pub filament_diameter_mm: f64,

    #[schemars(description = "Filament density in g/cm³.

Used to convert the extruded volume into a filament **weight** for the G-code
metadata header. Typical values:
- `1.24` — PLA
- `1.27` — PETG
- `1.04` — ABS", extend("x-group" = "Material", "x-unit" = "g_cm3", "x-tier" = "advanced"))]
    #[serde(default = "SlicingParams::default_filament_density_g_cm3")]
    pub filament_density_g_cm3: f64,

    #[schemars(description = "Filament price in currency units per kilogram.

Combined with the filament weight to report a material cost in the G-code
metadata footer. Populated from the active filament profile at resolve time.
`0` = unknown, which omits the cost line.", extend("x-group" = "Material", "x-unit" = "per_kg", "x-tier" = "advanced"))]
    #[serde(default)]
    pub filament_cost_per_kg: f64,

    #[schemars(description = "Nozzle orifice diameter in mm.

Affects minimum feature resolution and all line-width calculations.
**Standard:** 0.4 mm. Other common sizes: 0.2, 0.6, 0.8 mm.", extend("x-group" = "Hardware", "x-unit" = "mm", "x-step" = 0.01))]
    #[serde(default = "SlicingParams::default_nozzle_diameter_mm")]
    pub nozzle_diameter_mm: f64,

    #[schemars(
        description = "Vertical offset in mm added to every Z coordinate in the G-code.

Compensates for a Z endstop that does not zero exactly at the bed:
- **Negative** lowers the nozzle (endstop leaves it too high — a `0.3 mm` gap needs `-0.3`)
- **Positive** raises it (first layer is squashed)

Applies to the axis moves and the layer markers only; the model, the slice
layers and the print statistics are unchanged, and your own start/end G-code is
never rewritten. **Prefer fixing the endstop** — this is a compensation, not a
calibration. **Typical:** −0.1 to 0.1 mm. `0` = disabled.",
        extend("x-group" = "Hardware", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_z_offset_mm")]
    pub z_offset_mm: f64,

    #[schemars(description = "Build-plate surface type recorded in the G-code metadata header.

Free-form label (e.g. `Textured PEI Plate`, `Cool Plate`, `Engineering Plate`).
Purely informational: it is tracked for printer integration / diagnostics and
does **not** affect slicing. Empty = omit the `; bed_type:` header line.", extend("x-group" = "Hardware"))]
    #[serde(default)]
    pub bed_type: String,

    #[schemars(
        description = "Machine has an **actively heated** chamber.

A hardware capability, not a preference: it is what allows the filament's
`chamber_temp` to be emitted as a real heat directive (`M141`/`M191`, or Klipper's
`SET_HEATER_TEMPERATURE` / `TEMPERATURE_WAIT`). Leave it off for a passive
enclosure or a machine with no chamber heater — an unknown chamber command
aborts the print on Klipper.

`chamber_temp` still reaches custom start G-code as `{chamber_temp}` either way.",
        extend("x-group" = "Hardware", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_heated_chamber")]
    pub heated_chamber: bool,

    #[schemars(description = "Printer manufacturer recorded in the G-code metadata footer as \
`printer_vendor`.

Populated from the active printer profile at resolve time so Moonraker
(Mainsail / Fluidd) and OctoPrint can show which machine the file was sliced
for. Purely informational — it does **not** affect slicing. Empty = omit the \
line.", extend("x-group" = "Hardware"))]
    #[serde(default)]
    pub printer_vendor: String,

    #[schemars(description = "Printer model recorded in the G-code metadata footer as \
`printer_model`.

Populated from the active printer profile at resolve time. Printer front-ends
display it alongside the job, and some use it to warn when a file was sliced for
a different machine. Empty = omit the line.", extend("x-group" = "Hardware"))]
    #[serde(default)]
    pub printer_model: String,

    #[schemars(description = "Non-print (travel) move speed in **mm/min**.

Convert from mm/s by multiplying by 60. Fast travel reduces print time without affecting print quality.
**Example:** 9000 mm/min = 150 mm/s.", extend("x-group" = "Speed", "x-unit" = "mm_min", "x-tier" = "advanced"))]
    #[serde(default = "SlicingParams::default_travel_speed_mm_min")]
    pub travel_speed_mm_min: f64,

    #[schemars(
        description = "Retraction speed in **mm/min**.

Convert from mm/s by multiplying by 60.
**Example:** 2400 mm/min = 40 mm/s.",
        extend("x-group" = "Retraction", "x-unit" = "mm_min", "x-step" = 60, "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_retract_speed_mm_min")]
    pub retract_speed_mm_min: f64,

    #[schemars(description = "Z-hop lift height in mm during travel moves.

Lifts the nozzle before travelling to reduce stringing and nozzle drag across the print.
**Typical:** 0.2–0.5 mm. Set to `0` to disable.", extend("x-group" = "Retraction", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "advanced"))]
    #[serde(default = "SlicingParams::default_z_hop_mm")]
    pub z_hop_mm: f64,

    #[schemars(description = "Retraction distance in mm on travel moves.

Pulls filament back into the nozzle to reduce oozing and stringing.
**Typical:** 0.5–2 mm (direct drive) or 3–7 mm (Bowden).", extend("x-group" = "Retraction", "x-unit" = "mm", "x-step" = 0.05))]
    #[serde(default = "SlicingParams::default_retract_mm")]
    pub retract_mm: f64,

    #[schemars(description = "Minimum travel distance in mm before a retraction is triggered.

Short hops between adjacent paths do not ooze enough to justify the
retract → travel → un-retract cycle (which itself takes longer than the hop).
Travels longer than 2 mm always retract regardless of this value.
**Typical:** 1.0–2.0 mm. Set to `0` to retract on every travel.", extend("x-group" = "Retraction", "x-unit" = "mm", "x-step" = 0.05, "x-tier" = "advanced"))]
    #[serde(default = "SlicingParams::default_retract_before_travel_mm")]
    pub retract_before_travel_mm: f64,

    #[schemars(description = "Extra prime length in mm added when un-retracting after a travel.

Compensates for filament that oozed away during the travel by depositing a
little extra material on restart. In firmware retraction mode this is forwarded
to the firmware (`M208`/`SET_RETRACTION`).
**Typical:** 0.0–0.2 mm. Set to `0` to disable.", extend("x-group" = "Retraction", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "expert"))]
    #[serde(default = "SlicingParams::default_retract_restart_extra_mm")]
    pub retract_restart_extra_mm: f64,

    #[schemars(
        description = "Force a retraction at every layer change.

Retracts before the layer-change Z move so the nozzle does not ooze while
lifting and travelling to the first path of the next layer.
**Recommended:** off (the first travel of each layer already retracts).",
        extend("x-group" = "Retraction", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_retract_on_layer_change")]
    pub retract_on_layer_change: bool,

    #[schemars(
        description = "Use firmware retraction (`G10`/`G11`) instead of extruder-axis (`G1 E`) moves.

Delegates retraction to the printer firmware. The slicer emits `G10`/`G11` and
syncs the firmware's retraction length, speed and restart-extra from the
retraction settings (`M207`/`M208` on Marlin, `SET_RETRACTION` on Klipper).
Requires firmware retraction support (`[firmware_retraction]` on Klipper).
**Recommended:** off unless your firmware is configured for it.",
        extend("x-group" = "Retraction", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_use_firmware_retraction")]
    pub use_firmware_retraction: bool,

    #[schemars(
        description = "Emit relative extruder distances (`M83`) instead of absolute (`M82`).

Each extrusion move carries the incremental filament length rather than a
running absolute position. Relative E is more robust across custom start
G-code / macros that leave the extruder in an unknown state.
**Recommended:** off for maximum compatibility; on if your macros expect it.",
        extend("x-group" = "Retraction", "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_use_relative_e_distances")]
    pub use_relative_e_distances: bool,

    #[schemars(
        description = "Wipe the nozzle along the just-printed path while retracting.

Retraces the tail of the previous path before travelling, smearing any ooze
onto already-printed material instead of leaving a blob at the seam.
**Recommended:** off; enable to reduce stringing on some materials.",
        extend("x-group" = "Retraction", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_wipe")]
    pub wipe: bool,

    #[schemars(description = "Distance in mm to wipe the nozzle when `wipe` is enabled.

The nozzle retraces this far back along the just-printed path. Capped at the
length of that path.
**Typical:** 1.0–3.0 mm.", extend("x-group" = "Retraction", "x-unit" = "mm", "x-step" = 0.05, "x-tier" = "expert"))]
    #[serde(default = "SlicingParams::default_wipe_distance_mm")]
    pub wipe_distance_mm: f64,

    #[schemars(
        description = "Fraction of the retraction performed before the wipe move (0.0–1.0).

`0.0` performs the whole retraction *during* the wipe (distributed along the
wipe move); `1.0` retracts fully *before* wiping. Only used when `wipe` is
enabled and firmware retraction is off (firmware retraction cannot split a
retraction).
**Typical:** 0.0.",
        extend("x-group" = "Retraction", "x-tier" = "expert", "x-unit" = "fraction")
    )]
    #[serde(default = "SlicingParams::default_retract_before_wipe_percent")]
    pub retract_before_wipe_percent: f64,

    #[schemars(
        description = "Use a single outer wall on the topmost layer of top surfaces.

Reduces the chance of pillowing and prevents infill patterns from showing through the top surface.
**Recommended:** enabled.",
        extend("x-group" = "Surfaces", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_only_one_wall_top")]
    pub only_one_wall_top: bool,

    #[schemars(description = "Use a single outer wall on the first layer.

Improves bed adhesion and avoids potential issues with multiple perimeters pressing against the bed simultaneously.
**Recommended:** enabled.", extend("x-group" = "Surfaces", "x-tier" = "advanced"))]
    #[serde(default = "SlicingParams::default_only_one_wall_first_layer")]
    pub only_one_wall_first_layer: bool,

    #[schemars(
        description = "Overlap of solid surfaces into perimeter walls for bonding (0.0–1.0).

Ensures surfaces bond to walls without leaving gaps at the perimeter boundary.
**Typical:** 0.25 (25% of a bead width).",
        extend("x-group" = "Infill", "x-tier" = "advanced", "x-unit" = "fraction")
    )]
    #[serde(default = "SlicingParams::default_infill_overlap_percent")]
    pub infill_overlap_percent: f64,

    #[schemars(
        description = "Gap in mm between the innermost perimeter wall and sparse infill lines.

A small gap prevents infill from pressing too hard against walls, reducing surface artefacts
where the infill pattern shows through the outer wall.
**Typical:** 0.0–0.2 mm. Set to `0.0` for no gap (infill touches the inner wall).",
        extend("x-group" = "Infill", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_infill_perimeter_gap_mm")]
    pub infill_perimeter_gap_mm: f64,

    #[schemars(
        description = "Minimum infill line length in mm.

Scan-line segments shorter than this threshold are discarded instead of being
printed, for **both** solid top/bottom surface fill and sparse infill. Tiny
slivers at curved or diagonal boundaries — and isolated sparse-infill dashes in
narrow corners — waste printhead motion (a full retract/travel/un-retract for a
mechanically-insignificant dab) without meaningfully improving coverage;
adjacent lines and the flanking walls fill the space naturally.

Set to the nozzle diameter for best results (e.g. `0.4` for a 0.4 mm nozzle).
Set to `0.0` to disable the filter entirely.
**Default:** 0.4 mm (one standard nozzle diameter).",
        extend("x-group" = "Surfaces", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_min_infill_extrusion_mm")]
    pub min_infill_extrusion_mm: f64,

    #[schemars(
        description = "Maximum perpendicular deviation (mm) for path simplification (Ramer–Douglas–Peucker).

Reduces the number of G-code points without visibly affecting print quality.
**Typical:** 0.01–0.1 mm. Set to `0.0` to disable.",
        extend("x-group" = "Output", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_path_tolerance")]
    pub path_tolerance: f64,

    #[schemars(
        description = "G-code firmware flavor for the target printer.\n\nSupported values:\n- `marlin` — Marlin firmware (widely compatible)\n- `klipper` — Klipper firmware (macro-based)",
        extend("x-group" = "Output")
    )]
    #[serde(default = "SlicingParams::default_gcode_flavor")]
    pub gcode_flavor: GcodeFlavor,

    #[schemars(
        description = "Pause / color-change / custom G-code triggers, fired at exact layer boundaries or Z heights.

Each trigger fires once, immediately after the layer-change block for the
layer it targets (see `at_layer`/`at_z`), before that layer's geometry is
emitted. The firmware command emitted for `pause`/`color_change` depends on
`gcode_flavor` (Marlin: `M0`/`M600`; Klipper: `PAUSE`/macro; RepRap: `M226`).",
        extend("x-group" = "Output", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_triggers")]
    pub triggers: Vec<PauseTrigger>,

    #[schemars(
        description = "Fan configurations for layer-time-based adaptive cooling.

Each entry describes one physical fan in the printer. For multi-fan printers
(e.g. Bambu Lab X1C with 4 fans) add an entry per fan with the appropriate
`fan_index` (P0–P3).

Fan speed is adapted to the estimated layer print time:
- Short layers (< `layer_time_fast_s`): `max_speed`
- Long layers (> `layer_time_slow_s`): `min_speed`
- Between: smooth linear interpolation

**Default:** single part-cooling fan (P0) at 35%–100% speed.",
        extend("x-group" = "Cooling", "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_fan_configs")]
    pub fan_configs: Vec<FanConfig>,

    #[schemars(
        description = "Optional mesh decimation applied before slicing.

Supported values:
- `normal` — no decimation (default)
- `high-quality` — no decimation, signals maximum fidelity
- `draft` — aggressive polygon reduction for faster slicing",
        extend("x-group" = "Mesh", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_mesh_quality")]
    pub mesh_quality: MeshQuality,

    // --- Profile-contributed extensions -------------------------------------
    // These fields let printer / filament / process profiles express their full
    // intent (see `crate::profiles`).  Some are honoured by the pipeline today;
    // others are carried through and consumed by stub logic pending a real
    // implementation (marked `TODO(profiles): …` at the consumption site).
    #[schemars(
        description = "First-layer height in mm. `0` = use `layer_height`.

A thicker first layer improves bed adhesion.
**Typical:** 0.20–0.28 mm.",
        extend("x-group" = "Layer", "x-unit" = "mm", "x-step" = 0.01)
    )]
    #[serde(default = "SlicingParams::default_first_layer_height")]
    pub first_layer_height: f64,

    #[schemars(
        description = "Explicit extrusion line width in mm. `0` = derive from nozzle diameter.

Overrides the nozzle-derived default for solid infill and surfaces.
**Typical:** 100–120% of nozzle diameter.",
        extend("x-group" = "Walls", "x-unit" = "mm", "x-step" = 0.01)
    )]
    #[serde(default = "SlicingParams::default_line_width")]
    pub line_width: f64,

    #[schemars(
        description = "Per-role outer-wall extrusion width in mm. `0` = derive from \
`line_width` / nozzle diameter.

Overrides the width used for outer-wall paths and their `;TYPE:Outer wall` /
`;WIDTH:` G-code annotations. Ignored for variable-width Arachne beads, which
carry their own per-segment width.
**Typical:** 100–105% of nozzle diameter for dimensional accuracy.",
        extend("x-group" = "Walls", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_role_line_width")]
    pub outer_wall_line_width: f64,

    #[schemars(
        description = "Per-role inner-wall extrusion width in mm. `0` = derive from \
`line_width` / nozzle diameter.

Overrides the width used for inner-wall paths and their `;TYPE:Inner wall` /
`;WIDTH:` G-code annotations. Ignored for variable-width Arachne beads.
**Typical:** 110–120% of nozzle diameter for faster, stronger walls.",
        extend("x-group" = "Walls", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_role_line_width")]
    pub inner_wall_line_width: f64,

    #[schemars(
        description = "Per-role top/bottom solid-surface extrusion width in mm. `0` = \
derive from `line_width` / nozzle diameter.

Overrides the width used for top and bottom surface paths and their
`;TYPE:Top surface` / `;TYPE:Bottom surface` and `;WIDTH:` annotations.
**Typical:** 100% of nozzle diameter for a fine finish.",
        extend("x-group" = "Surfaces", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_role_line_width")]
    pub top_surface_line_width: f64,

    #[schemars(
        description = "Per-role sparse-infill extrusion width in mm. `0` = derive from \
`line_width` / nozzle diameter.

Overrides the width used for sparse-infill paths and their `;TYPE:Sparse infill`
/ `;WIDTH:` annotations.
**Typical:** 100–150% of nozzle diameter; wider infill prints faster.",
        extend("x-group" = "Infill", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_role_line_width")]
    pub sparse_infill_line_width: f64,

    #[schemars(
        description = "Global extrusion flow multiplier (0.0–2.0).

Scales every extrusion volume. `1.0` = nominal. Tune per-material to correct
under/over-extrusion.",
        extend("x-group" = "Extrusion", "x-unit" = "ratio")
    )]
    #[serde(default = "SlicingParams::default_flow_ratio")]
    pub flow_ratio: f64,

    #[schemars(
        description = "First-layer nozzle temperature in °C. `0` = use `nozzle_temp`.",
        extend("x-group" = "Temperature", "x-unit" = "celsius", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_nozzle_temp_first_layer")]
    pub nozzle_temp_first_layer: f64,

    #[schemars(
        description = "First-layer bed temperature in °C. `0` = use `bed_temp`.",
        extend("x-group" = "Temperature", "x-unit" = "celsius", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_bed_temp_first_layer")]
    pub bed_temp_first_layer: f64,

    #[schemars(
        description = "Chamber temperature in °C for enclosed printers. `0` = no active \
chamber heating.

Emitted as a real heat directive — the bed target is armed, then `M141`/`M191`
soak the chamber, all before the start G-code so the nozzle is still cold — but
**only when the printer profile sets `heated_chamber`**. Always available to
custom start G-code as `{chamber_temp}` (e.g. Klippain
`START_PRINT … CHAMBER={chamber_temp}`); a start script that heats the chamber
itself suppresses the automatic directives so the chamber is never heated twice.
**Typical:** 0 for PLA/PETG, 50–60 for ABS/ASA/PC.",
        extend("x-group" = "Temperature", "x-unit" = "celsius", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_chamber_temp")]
    pub chamber_temp: f64,

    #[schemars(
        description = "First-layer chamber temperature in °C. `0` = use `chamber_temp`.

A hotter initial soak helps the first layer bond on high-temperature materials;
the chamber drops back to `chamber_temp` once the first layer finishes.
Equivalent to OrcaSlicer's `chamber_temperature_initial_layer` and exposed to
custom start G-code as `{chamber_temp_first_layer}`.",
        extend("x-group" = "Temperature", "x-unit" = "celsius", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_chamber_temp_first_layer")]
    pub chamber_temp_first_layer: f64,

    #[schemars(
        description = "Material family name (e.g. `PLA`, `PETG`, `ABS`). Populated from the \
active filament profile at resolve time. Exposed to custom start G-code as `{filament_type}` \
(e.g. Klippain `START_PRINT … MATERIAL={filament_type}`).",
        extend("x-group" = "Material")
    )]
    #[serde(default)]
    pub filament_type: String,

    #[schemars(
        description = "Filament display name recorded in the G-code metadata footer as \
`filament_settings_id`. Populated from the active filament profile at resolve time so \
Moonraker / Mainsail / Fluidd, OctoPrint, and other front-ends can show which filament the \
file was sliced for. Empty = omit the line.",
        extend("x-group" = "Material")
    )]
    #[serde(default)]
    pub filament_name: String,

    #[schemars(
        description = "Filament colour (hex string, e.g. `#E0730F`) recorded in the G-code \
metadata footer as `filament_colour`. Populated from the active filament profile at resolve \
time so printer front-ends can render a swatch for the file. Empty = omit the line.",
        extend("x-group" = "Material")
    )]
    #[serde(default)]
    pub filament_color: String,

    #[schemars(
        description = "Linear/pressure advance factor (Klipper `SET_PRESSURE_ADVANCE`, Marlin `M900 K`).

`0` disables. Compensates for pressure lag at corners.
**Typical:** 0.02–0.08.",
        extend("x-group" = "Extrusion", "x-unit" = "s", "x-step" = 0.01, "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_pressure_advance")]
    pub pressure_advance: f64,

    #[schemars(
        description = "Print acceleration for normal moves in mm/s². `0` disables acceleration control.

When set, the slicer emits a firmware acceleration command whenever the target
changes (Klipper `SET_VELOCITY_LIMIT ACCEL=…`, Marlin `M204 P…`). Lower values
smooth motion and reduce ringing; higher values print faster.
**Typical:** 3000–10000.",
        extend("x-group" = "Speed", "x-unit" = "mm_s2", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_acceleration")]
    pub acceleration: f64,

    #[schemars(
        description = "First-layer acceleration in mm/s². `0` = use `acceleration`.

A lower first-layer acceleration improves bed adhesion by giving the nozzle more
dwell time. Applies to every role on the first layer.
**Typical:** 1000–3000.",
        extend("x-group" = "Speed", "x-unit" = "mm_s2", "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_first_layer_acceleration")]
    pub first_layer_acceleration: f64,

    #[schemars(
        description = "Top-surface acceleration in mm/s². `0` = use `acceleration`.

Solid top surfaces benefit from a distinct (often higher) acceleration for a
smoother finish. Applies to top-surface solid infill.
**Typical:** 5000–10000.",
        extend("x-group" = "Speed", "x-unit" = "mm_s2", "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_top_surface_acceleration")]
    pub top_surface_acceleration: f64,

    #[schemars(
        description = "Outer-wall acceleration in mm/s². `0` = use `acceleration`.

The outermost perimeter defines the visible surface; a lower, dedicated
acceleration reduces ringing and ghosting on external walls.
**Typical:** 2000–6000.",
        extend("x-group" = "Speed", "x-unit" = "mm_s2", "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_outer_wall_acceleration")]
    pub outer_wall_acceleration: f64,

    #[schemars(
        description = "Bridge / overhang acceleration in mm/s². `0` = use `acceleration`.

Strands printed into air (bridge infill and overhang perimeters) cool and sag
without support below; a low acceleration keeps flow steady and lets each strand
tension before the nozzle moves on.
**Typical:** 1000–3000.",
        extend("x-group" = "Speed", "x-unit" = "mm_s2", "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_bridge_acceleration")]
    pub bridge_acceleration: f64,

    #[schemars(
        description = "Inner-wall acceleration in mm/s². `0` = use `acceleration`.

Inner perimeters are hidden inside the part, so they can accelerate harder than
the visible outer wall. Applies to inner-wall paths only.
**Typical:** 4000–10000.",
        extend("x-group" = "Speed", "x-unit" = "mm_s2", "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_inner_wall_acceleration")]
    pub inner_wall_acceleration: f64,

    #[schemars(
        description = "Sparse-infill acceleration in mm/s². `0` = use `acceleration`.

Sparse infill is interior and invisible, so it usually gets the highest
acceleration to save print time. Applies to the sparse infill pattern.
**Typical:** 5000–12000.",
        extend("x-group" = "Speed", "x-unit" = "mm_s2", "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_sparse_infill_acceleration")]
    pub sparse_infill_acceleration: f64,

    #[schemars(
        description = "Solid-infill acceleration in mm/s². `0` = use `acceleration`.

Internal solid layers and bottom surfaces (distinct from the visible top
surface). Applies to bottom-surface and internal solid infill.
**Typical:** 4000–10000.",
        extend("x-group" = "Speed", "x-unit" = "mm_s2", "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_solid_infill_acceleration")]
    pub solid_infill_acceleration: f64,

    #[schemars(
        description = "Gap-fill acceleration in mm/s². `0` = use `acceleration`.

Gap fill lays short, variable-width beads that a lower acceleration keeps clean.
Applies to gap-fill paths.
**Typical:** 1000–4000.",
        extend("x-group" = "Speed", "x-unit" = "mm_s2", "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_gap_fill_acceleration")]
    pub gap_fill_acceleration: f64,

    #[schemars(
        description = "Support acceleration in mm/s². `0` = use `acceleration`.

Support material is sacrificial, so it can run fast. Applies to support paths.
**Typical:** 4000–10000.",
        extend("x-group" = "Speed", "x-unit" = "mm_s2", "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_support_acceleration")]
    pub support_acceleration: f64,

    #[schemars(
        description = "Travel (non-printing) acceleration in mm/s². `0` = use `acceleration`.

Travel moves deposit no material, so they can accelerate hard to cut the time
spent hopping between paths. When set, the slicer switches the firmware
acceleration to this value before each travel and back to the printing value
before extruding, and the print-time estimate follows the same switch.
**Typical:** 6000–15000.",
        extend("x-group" = "Speed", "x-unit" = "mm_s2", "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_travel_acceleration")]
    pub travel_acceleration: f64,

    #[schemars(
        description = "Square-corner velocity in mm/s — the speed the head keeps through a 90° corner (junction-deviation cornering). `0` = use the estimator/firmware default (5 mm/s).

Higher values corner faster (shorter prints, more ringing); lower values slow into corners for cleaner edges. When set, the slicer emits the firmware limit (Klipper `SET_VELOCITY_LIMIT SQUARE_CORNER_VELOCITY=…`, Marlin `M205 J…` junction deviation) and the print-time estimate uses the same value, so the ETA tracks reality.
**Typical:** 5–10.",
        extend("x-group" = "Speed", "x-unit" = "mm_s", "x-step" = 1, "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_square_corner_velocity")]
    pub square_corner_velocity: f64,

    #[schemars(
        description = "Maximum travel/print velocity cap in mm/s. `0` = unlimited (no cap).

The machine's top speed: any role feedrate above this is clamped by the firmware, so the estimate honors it too. When set, the slicer emits the firmware limit (Klipper `SET_VELOCITY_LIMIT VELOCITY=…`, Marlin `M203 X… Y…`).
**Typical:** 150–500.",
        extend("x-group" = "Speed", "x-unit" = "mm_s", "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_max_velocity")]
    pub max_velocity: f64,

    #[schemars(
        description = "Fixed warm-up allowance in seconds added *before* the toolpath in the print-time estimate. `0` = none.

Accounts for wall-clock the toolpath can't show — homing, bed mesh, heat-soak, purge — none of which is derivable from the moves. A flat allowance, not a thermal model.
**Typical:** 60–300.",
        extend("x-group" = "Time estimate", "x-unit" = "s", "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_time_estimate_warmup_s")]
    pub time_estimate_warmup_s: f64,

    #[schemars(
        description = "Fixed cool-down allowance in seconds added *after* the toolpath in the print-time estimate. `0` = none.

For material/hardware that isn't \"done\" at the last move — e.g. an ABS chamber cool-off or a park-and-cool end sequence — before the print is truly finished.
**Typical:** 0–120.",
        extend("x-group" = "Time estimate", "x-unit" = "s", "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_time_estimate_cooldown_s")]
    pub time_estimate_cooldown_s: f64,

    #[schemars(
        description = "Calibration multiplier applied to the *toolpath* portion of the print-time estimate. `1.0` = no adjustment.

If real prints consistently run a few percent over/under the estimate (tiny details the model rounds off, firmware smoothing), nudge this to match your machine — `1.05` adds 5 %. Scales only the toolpath; the fixed warm-up/cool-down allowances are added afterward.
**Typical:** 0.9–1.15.",
        extend("x-group" = "Time estimate", "x-tier" = "expert", "x-unit" = "ratio")
    )]
    #[serde(default = "SlicingParams::default_time_estimate_scale")]
    pub time_estimate_scale: f64,

    #[schemars(
        description = "Number of initial layers with the part-cooling fan forced off.

Improves adhesion of the first few layers.
**Typical:** 1.",
        extend("x-group" = "Cooling", "x-unit" = "count", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_disable_fan_first_layers")]
    pub disable_fan_first_layers: usize,

    #[schemars(
        description = "Maximum volumetric extrusion rate in mm³/s. `0` = unlimited.

Caps print speed so the hotend can keep up with the flow.
**Typical:** 8–24 mm³/s depending on material and hotend.",
        extend("x-group" = "Extrusion", "x-unit" = "mm3_s", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_max_volumetric_speed")]
    pub max_volumetric_speed: f64,

    #[schemars(
        description = "Number of extruders (tools) on the machine. Multi-material is not yet supported.",
        extend("x-group" = "Hardware", "x-unit" = "count", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_extruder_count")]
    pub extruder_count: usize,

    #[schemars(
        description = "Whether support structures are generated.",
        extend("x-group" = "Support")
    )]
    #[serde(default = "SlicingParams::default_support_enabled")]
    pub support_enabled: bool,

    #[schemars(
        description = "Detect overhangs automatically, as well as honouring painted regions.

On (the default), overhangs steeper than the threshold angle get support and painted areas add to \
or subtract from that. Turn it **off** to place support *only* where you painted it — the overhang \
rule is skipped entirely, so nothing is generated that you did not ask for. Painted blockers still \
apply either way.",
        extend("x-group" = "Support", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "support_enabled", "equals": true}))
    )]
    #[serde(default = "SlicingParams::default_support_auto")]
    pub support_auto: bool,

    #[schemars(
        description = "How support is built under an overhang.

The threshold angle, density, interface layers and XY/Z clearance below apply
to both styles.",
        extend("x-group" = "Support", "x-widget" = "cards", "x-relevant-when" = serde_json::json!({"field": "support_enabled", "equals": true}))
    )]
    #[serde(default)]
    pub support_type: SupportType,

    #[schemars(
        description = "Overhang angle threshold in degrees, measured from vertical (0–89).

Any surface that overhangs more steeply than this gets support beneath it. `45°` is the classic
rule (a wall may step outward by one layer height per layer); a **smaller** angle is more
conservative and supports gentler overhangs, a **larger** angle supports only severe overhangs.
**Default:** 45°.",
        extend("x-group" = "Support", "x-unit" = "deg", "x-relevant-when" = serde_json::json!({"field": "support_enabled", "equals": true}))
    )]
    #[serde(default = "SlicingParams::default_support_threshold_angle")]
    pub support_threshold_angle: f64,

    #[schemars(
        description = "Support infill density as a fraction (0.0–1.0).

Controls the line spacing of the support body (`spacing = extrusion_width / density`).
Lower is faster to print and easier to remove; higher is sturdier.",
        extend("x-group" = "Support", "x-tier" = "advanced", "x-unit" = "fraction", "x-relevant-when" = serde_json::json!({"field": "support_enabled", "equals": true}))
    )]
    #[serde(default = "SlicingParams::default_support_density")]
    pub support_density: f64,

    #[schemars(
        description = "Number of dense interface layers between the support body and the model \
(both the top contact under an overhang and the bottom contact resting on the model).

Denser contact layers give a cleaner overhang surface and detach more easily.
Set to `0` to disable interface layers (support body fills the whole column).",
        extend("x-group" = "Support", "x-unit" = "count", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "support_enabled", "equals": true}))
    )]
    #[serde(default = "SlicingParams::default_support_interface_layers")]
    pub support_interface_layers: usize,

    #[schemars(
        description = "Interface-layer fill density as a fraction (0.0–1.0).

The top/bottom contact layers are filled at this (usually higher) density for a smoother
overhang surface. Typically 0.6–0.9.",
        extend("x-group" = "Support", "x-tier" = "advanced", "x-unit" = "fraction", "x-relevant-when" = serde_json::json!({"field": "support_enabled", "equals": true}))
    )]
    #[serde(default = "SlicingParams::default_support_interface_density")]
    pub support_interface_density: f64,

    #[schemars(
        description = "Horizontal clearance in millimetres between support structures and the model \
walls (XY distance).

Larger values make supports easier to remove but reduce how well they hold up steep overhangs, and
leave a visible gap around the part. **Default:** 0.35 mm; 0.2–0.5 mm covers most machines.",
        extend("x-group" = "Support", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "support_enabled", "equals": true}))
    )]
    #[serde(default = "SlicingParams::default_support_xy_distance_mm")]
    pub support_xy_distance_mm: f64,

    #[schemars(
        description = "Vertical gap in layers between the top of a support column and the overhang \
it supports (Z distance, expressed in layers).

A gap of 1–2 layers leaves a small air pocket so the support detaches cleanly without fusing to
the model. Set to `0` for supports that touch the overhang directly (strongest, hardest to remove) —
note that the first model layer above support is still printed with bridging speed and cooling
either way.",
        extend("x-group" = "Support", "x-unit" = "count", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "support_enabled", "equals": true}))
    )]
    #[serde(default = "SlicingParams::default_support_z_gap_layers")]
    pub support_z_gap_layers: usize,

    #[schemars(
        description = "Only support overhangs that can be reached from the build plate.

Support is normally allowed to rest on the model itself. With this on, a column is kept only when it
can descend to the plate through empty space — anything that would land on the print is dropped, so
those overhangs print unsupported. Choose this when supports resting on the model would scar a
surface you care about, or would be impossible to remove.

A painted enforcer is exempt: it is a direct instruction to support that exact spot, so it keeps its
support even if the column has to rest on the model instead of reaching the plate.",
        extend("x-group" = "Support", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "support_enabled", "equals": true}))
    )]
    #[serde(default = "SlicingParams::default_support_on_build_plate_only")]
    pub support_on_build_plate_only: bool,

    #[schemars(
        description = "Per-role support extrusion width in mm. `0` = derive from \
`line_width` / nozzle diameter.

Overrides the width used for support strands and their `;WIDTH:` annotations. Support is laid at
`spacing / density`, so this sets the pitch and the flow together — a wider bead spends less time
per unit area but is coarser to break off.",
        extend("x-group" = "Support", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "expert", "x-relevant-when" = serde_json::json!({"field": "support_enabled", "equals": true}))
    )]
    #[serde(default = "SlicingParams::default_role_line_width")]
    pub support_line_width: f64,

    #[schemars(
        description = "What is printed around or under the object to hold it to the build plate.

Each choice reveals its own settings below.",
        extend("x-group" = "Adhesion", "x-widget" = "cards")
    )]
    #[serde(default)]
    pub adhesion_type: AdhesionType,

    #[schemars(
        description = "Brim width in mm (when `adhesion_type = brim`).",
        extend("x-group" = "Adhesion", "x-unit" = "mm", "x-step" = 0.05, "x-relevant-when" = serde_json::json!({"field": "adhesion_type", "equals": "brim"}))
    )]
    #[serde(default = "SlicingParams::default_brim_width")]
    pub brim_width: f64,

    #[schemars(
        description = "Where brim material is placed: `outer_only`, `inner_only`, `outer_and_inner`, or `ears`.",
        extend("x-group" = "Adhesion", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "adhesion_type", "equals": "brim"}))
    )]
    #[serde(default)]
    pub brim_type: BrimType,

    #[schemars(
        description = "Gap in mm between the brim and the object's first-layer contour (a.k.a. brim separation / offset). `0` fuses the brim directly onto the wall.",
        extend("x-group" = "Adhesion", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "adhesion_type", "equals": "brim"}))
    )]
    #[serde(default = "SlicingParams::default_brim_separation")]
    pub brim_separation: f64,

    #[schemars(
        description = "Number of skirt loops (when `adhesion_type = skirt`).",
        extend("x-group" = "Adhesion", "x-unit" = "count", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "adhesion_type", "equals": "skirt"}))
    )]
    #[serde(default = "SlicingParams::default_skirt_loops")]
    pub skirt_loops: usize,

    #[schemars(
        description = "Gap in mm between the object and the innermost skirt loop.",
        extend("x-group" = "Adhesion", "x-unit" = "mm", "x-step" = 0.05, "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "adhesion_type", "equals": "skirt"}))
    )]
    #[serde(default = "SlicingParams::default_skirt_distance")]
    pub skirt_distance: f64,

    #[schemars(
        description = "Number of layers the skirt spans (≥1). Values >1 act as a draft shield around the print.",
        extend("x-group" = "Adhesion", "x-unit" = "count", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "adhesion_type", "equals": "skirt"}))
    )]
    #[serde(default = "SlicingParams::default_skirt_height")]
    pub skirt_height: usize,

    #[schemars(
        description = "Raft layer count: sacrificial base + interface layers printed under the object (when `adhesion_type = raft`, or any value >0).",
        extend("x-group" = "Adhesion", "x-unit" = "count", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "adhesion_type", "equals": "raft"}))
    )]
    #[serde(default = "SlicingParams::default_raft_layers")]
    pub raft_layers: usize,

    #[schemars(
        description = "Vertical air gap in mm between the top of the raft and the object's first layer (eases raft removal).",
        extend("x-group" = "Adhesion", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "adhesion_type", "equals": "raft"}))
    )]
    #[serde(default = "SlicingParams::default_raft_air_gap")]
    pub raft_air_gap: f64,

    #[schemars(
        description = "Grow (positive) or shrink (negative) every contour by this many mm, on every layer.

Corrects a printer that consistently prints parts slightly over- or under-sized.
Because the material expands inward as well as outward, a positive value also
makes holes *smaller* — use `xy_hole_compensation` to correct holes separately.
**Typical:** -0.1 to 0.1. `0` = off.",
        extend("x-group" = "Quality", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_xy_size_compensation")]
    pub xy_size_compensation: f64,

    #[schemars(
        description = "Enlarge (positive) or tighten (negative) holes by this many mm, on every layer.

Applied after `xy_size_compensation` and only to enclosed voids, so a press-fit
hole can be opened up without changing the part's outside dimensions. **Typical:**
0 to 0.1. `0` = off.",
        extend("x-group" = "Quality", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_xy_hole_compensation")]
    pub xy_hole_compensation: f64,

    #[schemars(
        description = "Inward shrink in mm applied to the layers nearest the bed. `0` = off.

The first layer is squashed into the build plate, so it spreads outward and the
base of the print measures oversize — the \"elephant foot\". This shrinks it back.

Unlike `xy_size_compensation` this is **not** a uniform offset: the shrink is
limited by how thin the geometry is at each point, so embossed text, logo strokes
and thin ribs on the first layer keep at least
`elephant_foot_min_contour_width_mm` of width instead of being erased. It is also
withheld where the model itself flares steeply outward, so a narrow base under a
wide body is never undercut, and it is skipped entirely when printing on a raft.

**Typical:** 0.10-0.20 mm. Measure the bulge with calipers and halve it.",
        extend("x-group" = "Quality", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_elephant_foot_compensation_mm")]
    pub elephant_foot_compensation_mm: f64,

    #[schemars(
        description = "Number of layers the elephant-foot shrink is spread over (>=1).

`1` corrects the first layer only - the sharpest correction, and the right
default, because only the first layer is squashed. Higher values ramp the shrink
linearly to zero over that many layers, trading a little accuracy for a gentler
profile when a large correction would otherwise leave a visible step.
**Typical:** 1-3.",
        extend("x-group" = "Quality", "x-unit" = "count", "x-tier" = "expert", "x-relevant-when" = serde_json::json!({"field": "elephant_foot_compensation_mm", "greaterThan": 0.0}))
    )]
    #[serde(default = "SlicingParams::default_elephant_foot_layers")]
    pub elephant_foot_layers: usize,

    #[schemars(
        description = "Width in mm that elephant-foot compensation may never shrink a feature below. \
`0` = automatic (1.5 x outer-wall width).

This is what stops the correction deleting fine first-layer detail. A feature
already at or below this width is left completely alone; wider ones are shrunk
only as far as this width. Raise it to protect chunkier detail, lower it for a
more literal correction.
**Typical:** 0 (automatic), or 0.5-1.0 mm.",
        extend("x-group" = "Quality", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "expert", "x-relevant-when" = serde_json::json!({"field": "elephant_foot_compensation_mm", "greaterThan": 0.0}))
    )]
    #[serde(default = "SlicingParams::default_elephant_foot_min_contour_width_mm")]
    pub elephant_foot_min_contour_width_mm: f64,

    #[schemars(
        description = "Iron top surfaces for a smoother finish.

A second, nearly dry pass that re-melts the surface and spreads it flat. Slow —
it re-traverses the whole surface at a fraction of the fill spacing — so it is
worth it on visible flat tops and little else.",
        extend("x-group" = "Surfaces", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_ironing_enabled")]
    pub ironing_enabled: bool,

    #[schemars(
        description = "Which surfaces the ironing pass sweeps.",
        extend("x-group" = "Surfaces", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "ironing_enabled", "equals": true}))
    )]
    #[serde(default)]
    pub ironing_type: IroningType,

    #[schemars(
        description = "Material deposited during ironing, as a percentage of a normal solid-fill bead.

Just enough to re-melt the surface without adding height. **Typical:** 8–15 %.",
        extend("x-group" = "Surfaces", "x-tier" = "advanced", "x-unit" = "percent", "x-relevant-when" = serde_json::json!({"field": "ironing_enabled", "equals": true}))
    )]
    #[serde(default = "SlicingParams::default_ironing_flow")]
    pub ironing_flow: f64,

    #[schemars(
        description = "Distance in mm between adjacent ironing passes.

Much finer than the fill spacing — this is what flattens the ridges between
beads. **Typical:** 0.1–0.2.",
        extend("x-group" = "Surfaces", "x-unit" = "mm", "x-step" = 0.01, "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "ironing_enabled", "equals": true}))
    )]
    #[serde(default = "SlicingParams::default_ironing_spacing")]
    pub ironing_spacing: f64,

    #[schemars(
        description = "Ironing speed in mm/s. `0` = fall back to the top-surface speed.

Slow is the point: the nozzle needs dwell time to re-melt what it passes over.
**Typical:** 15–30 mm/s.",
        extend("x-group" = "Surfaces", "x-unit" = "mm_s", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "ironing_enabled", "equals": true}))
    )]
    #[serde(default = "SlicingParams::default_ironing_speed")]
    pub ironing_speed: f64,

    #[schemars(
        description = "Ironing sweep direction in degrees. `-1` = follow the layer's own surface fill angle.

Set an explicit angle to cross the fill direction, which flattens the ridges
more effectively than ironing along them.",
        extend("x-group" = "Surfaces", "x-unit" = "deg", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "ironing_enabled", "equals": true}))
    )]
    #[serde(default = "SlicingParams::default_ironing_angle")]
    pub ironing_angle: f64,

    #[schemars(
        description = "Custom start G-code block, inserted before the first print move. `null` = flavor default.",
        extend("x-group" = "Output", "x-tier" = "advanced", "x-widget" = "gcode")
    )]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_gcode: Option<String>,

    #[schemars(
        description = "Custom end G-code block, inserted after the last print move. `null` = flavor default.",
        extend("x-group" = "Output", "x-tier" = "advanced", "x-widget" = "gcode")
    )]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_gcode: Option<String>,

    #[schemars(
        description = "Custom G-code block inserted at every layer change, after the Z move. \
                       Supports `{z}`, `{height}`, and `{layer_num}` (1-based) placeholders. \
                       `null` = none.",
        extend("x-group" = "Output", "x-tier" = "advanced", "x-widget" = "gcode")
    )]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_gcode: Option<String>,

    #[schemars(
        description = "Per-filament start G-code, inserted after the machine start G-code and \
                       before the first print move. Typically supplied by the filament profile \
                       (temperatures, purge, pressure advance). Supports the same temperature / \
                       material placeholders as `start_gcode`. `null` = none.",
        extend("x-group" = "Filament G-code", "x-tier" = "advanced", "x-widget" = "gcode")
    )]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_filament_gcode: Option<String>,

    #[schemars(
        description = "Per-filament end G-code, inserted after the last print move and before the \
                       machine end G-code. Typically supplied by the filament profile. Supports \
                       the same temperature / material placeholders as `end_gcode`. `null` = none.",
        extend("x-group" = "Filament G-code", "x-tier" = "advanced", "x-widget" = "gcode")
    )]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_filament_gcode: Option<String>,

    #[schemars(
        description = "Embed a PNG thumbnail comment block in generated G-code files. \
                       The UI renders it from a fixed camera angle and theme (see \
                       `thumbnail_view` / `thumbnail_theme`) when slicing.",
        extend("x-group" = "Thumbnail", "x-tier" = "advanced")
    )]
    #[serde(default = "SlicingParams::default_thumbnail_enabled")]
    pub thumbnail_enabled: bool,

    #[schemars(
        description = "Square thumbnail resolution in pixels — this is the thumbnail's quality knob.",
        extend("x-group" = "Thumbnail", "x-unit" = "px", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "thumbnail_enabled", "equals": true}))
    )]
    #[serde(default = "SlicingParams::default_thumbnail_size_px")]
    pub thumbnail_size_px: u32,

    #[schemars(
        description = "Fixed camera angle used to render the embedded thumbnail (not the live view).",
        extend("x-group" = "Thumbnail", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "thumbnail_enabled", "equals": true}))
    )]
    #[serde(default)]
    pub thumbnail_view: ThumbnailView,

    #[schemars(
        description = "Fixed colour scheme for the embedded thumbnail — independent of the app/OS theme.",
        extend("x-group" = "Thumbnail", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "thumbnail_enabled", "equals": true}))
    )]
    #[serde(default)]
    pub thumbnail_theme: ThumbnailTheme,

    #[schemars(
        description = "How the model is coloured in the thumbnail: a neutral grey, the active \
                       filament's colour, or a specific colour you choose.",
        extend("x-group" = "Thumbnail", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "thumbnail_enabled", "equals": true}))
    )]
    #[serde(default)]
    pub thumbnail_color_mode: ThumbnailColorMode,

    #[schemars(
        description = "Specific model colour (`#rrggbb`) used when the thumbnail colour mode is `custom`.",
        extend("x-group" = "Thumbnail", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "thumbnail_color_mode", "equals": "custom"}))
    )]
    #[serde(default = "SlicingParams::default_thumbnail_custom_color")]
    pub thumbnail_custom_color: String,

    /// Optional base64-encoded PNG payload for the current slice request.
    ///
    /// This is an ephemeral request-scoped value (not a user-facing setting),
    /// intentionally excluded from JSON schema so the settings UI does not
    /// render it as an editable field.
    #[schemars(skip)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_png_base64: Option<String>,

    #[schemars(
        description = "Whether the plate is printed as one job or one object at a time.

Finishing objects one by one keeps a failure from spoiling the rest of the
plate, but the printhead has to travel over finished parts — so the printhead
clearance settings on the Printer tab decide which layouts are safe to run.",
        extend("x-group" = "Objects", "x-tier" = "advanced", "x-widget" = "cards")
    )]
    #[serde(default)]
    pub print_sequence: PrintSequence,

    #[schemars(
        description = "Let a single object be cancelled while the print continues, if it fails or lifts off the bed — without losing the rest of the plate. Needs a printer that supports skipping objects.",
        extend("x-group" = "Hardware", "x-tier" = "advanced")
    )]
    #[serde(default)]
    pub exclude_object: bool,

    #[schemars(
        description = "Clearance height in mm: an object shorter than this fits under the printhead as it moves. Used when printing objects one at a time to warn before a tall part is left in the printhead's path.",
        extend("x-group" = "Hardware", "x-unit" = "mm", "x-step" = 1, "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_extruder_clearance_height")]
    pub extruder_clearance_height_mm: f64,

    #[schemars(
        description = "How far the printhead and its fan shroud reach out around the nozzle, in mm. Used when printing objects one at a time to warn before two parts are placed too close to reach safely.",
        extend("x-group" = "Hardware", "x-unit" = "mm", "x-step" = 1, "x-tier" = "expert")
    )]
    #[serde(default = "SlicingParams::default_extruder_clearance_radius")]
    pub extruder_clearance_radius_mm: f64,

    #[schemars(
        description = "Custom G-code to run after one object is finished and before the next one starts, when printing objects one at a time. Leave empty for none.",
        extend("x-group" = "Objects", "x-tier" = "expert", "x-widget" = "gcode", "x-relevant-when" = serde_json::json!({"field": "print_sequence", "equals": "by_object"}))
    )]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub between_objects_gcode: Option<String>,

    #[schemars(
        description = "Whether the slicer emits a bed mesh command at print start.

Off by default: most printers already level inside their own start macro or a
one-time manual calibration, and a slicer-driven command on top of that would
duplicate the work or race it.",
        extend("x-group" = "Hardware", "x-tier" = "advanced", "x-widget" = "cards")
    )]
    #[serde(default)]
    pub bed_mesh_mode: BedMeshMode,

    #[schemars(
        description = "Named mesh profile to load, e.g. Klipper's `SAVE_CONFIG`-persisted profile \
                       name. `null` = the printer's default/active profile.",
        extend("x-group" = "Hardware", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "bed_mesh_mode", "equals": "load_profile"}))
    )]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bed_mesh_profile_name: Option<String>,

    #[schemars(
        description = "Bound the recalibration area to the print's XY footprint instead of the \
                       full bed, where the firmware supports it (Klipper `BED_MESH_CALIBRATE \
                       AREA_MIN=.. AREA_MAX=..`). Skips probing points the print never covers.",
        extend("x-group" = "Hardware", "x-tier" = "advanced", "x-relevant-when" = serde_json::json!({"field": "bed_mesh_mode", "equals": "calibrate"}))
    )]
    #[serde(default)]
    pub bed_mesh_adaptive: bool,
}

/// Schema helper: emit the full [`SlicingParams`] schema for a
/// `serde_json::Value` field that carries a sparse `SlicingParams` overlay.
///
/// Used via `#[schemars(schema_with = "...")]` on the profile `params` bags so
/// the UI can discover valid keys, groups, and relevance metadata.
pub fn slicing_params_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    generator.subschema_for::<SlicingParams>()
}

impl Default for SlicingParams {
    /// Sensible defaults for a standard PLA print.
    fn default() -> Self {
        Self {
            layer_height: 0.2,
            wall_generator: Self::default_wall_generator(),
            wall_count: Self::default_wall_count(),
            wall_line_width_min: Self::default_wall_line_width_min(),
            wall_line_width_max: Self::default_wall_line_width_max(),
            wall_transition_threshold: Self::default_wall_transition_threshold(),
            wall_transition_length: Self::default_wall_transition_length(),
            wall_distribution_count: Self::default_wall_distribution_count(),
            wall_transition_angle: Self::default_wall_transition_angle(),
            wall_transition_filter_distance: Self::default_wall_transition_filter_distance(),
            seam_position: Self::default_seam_position(),
            external_perimeters_first: Self::default_external_perimeters_first(),
            extra_perimeters: Self::default_extra_perimeters(),
            extra_perimeters_max_gap: Self::default_extra_perimeters_max_gap(),
            thin_walls: Self::default_thin_walls(),
            ensure_vertical_shell_thickness: Self::default_ensure_vertical_shell_thickness(),
            avoid_crossing_perimeters: Self::default_avoid_crossing_perimeters(),
            spiral_vase: Self::default_spiral_vase(),
            fuzzy_skin: Self::default_fuzzy_skin(),
            fuzzy_skin_thickness_mm: Self::default_fuzzy_skin_thickness_mm(),
            fuzzy_skin_point_dist_mm: Self::default_fuzzy_skin_point_dist_mm(),
            infill_density: 0.15,
            infill_pattern: Self::default_infill_pattern(),
            infill_base_angle: Self::default_infill_base_angle(),
            infill_anchor_percent: Self::default_infill_anchor_percent(),
            infill_anchor_max_mm: Self::default_infill_anchor_max_mm(),
            infill_every_layers: Self::default_infill_every_layers(),
            infill_combination_max_layer_height_mm:
                Self::default_infill_combination_max_layer_height_mm(),
            solid_infill_every_layers: Self::default_solid_infill_every_layers(),
            print_speed: 120.0,
            perimeter_speed: Self::default_perimeter_speed(),
            infill_speed: Self::default_infill_speed(),
            bridge_speed: Self::default_bridge_speed(),
            enable_overhang_speed: Self::default_enable_overhang_speed(),
            overhang_1_4_speed: Self::default_overhang_degree_speed(),
            overhang_2_4_speed: Self::default_overhang_degree_speed(),
            overhang_3_4_speed: Self::default_overhang_degree_speed(),
            overhang_4_4_speed: Self::default_overhang_4_4_speed(),
            slowdown_for_curled_perimeters: Self::default_slowdown_for_curled_perimeters(),
            bridge_flow_ratio: Self::default_bridge_flow_ratio(),
            bridge_min_area_mm2: Self::default_bridge_min_area_mm2(),
            bridge_noise_filter_mm: Self::default_bridge_noise_filter_mm(),
            bridge_anchor_mm: Self::default_bridge_anchor_mm(),
            bridge_angle: Self::default_bridge_angle(),
            top_surface_speed: Self::default_top_surface_speed(),
            gap_fill_speed: Self::default_gap_fill_speed(),
            support_speed: Self::default_support_speed(),
            gap_fill_min_length_mm: Self::default_gap_fill_min_length_mm(),
            wall_overlap_compensation: Self::default_wall_overlap_compensation(),
            first_layer_speed: Self::default_first_layer_speed(),
            fan_speed: Self::default_fan_speed(),
            bridge_fan_speed: Self::default_bridge_fan_speed(),
            overhang_fan_speed: Self::default_overhang_fan_speed(),
            overhang_fan_threshold: Self::default_overhang_fan_threshold(),
            first_layer_fan_speed: Self::default_first_layer_fan_speed(),
            min_layer_time_s: Self::default_min_layer_time_s(),
            min_print_speed: Self::default_min_print_speed(),
            coasting_distance_mm: Self::default_coasting_distance_mm(),
            nozzle_temp: 210.0,
            bed_temp: 60.0,
            top_layers: Self::default_top_layers(),
            bottom_layers: Self::default_bottom_layers(),
            surface_infill_angle: Self::default_surface_infill_angle(),
            top_surface_pattern: Self::default_top_surface_pattern(),
            bottom_surface_pattern: Self::default_bottom_surface_pattern(),
            internal_solid_infill_pattern: Self::default_internal_solid_infill_pattern(),
            filament_diameter_mm: Self::default_filament_diameter_mm(),
            filament_density_g_cm3: Self::default_filament_density_g_cm3(),
            filament_cost_per_kg: 0.0,
            nozzle_diameter_mm: Self::default_nozzle_diameter_mm(),
            z_offset_mm: Self::default_z_offset_mm(),
            bed_type: String::new(),
            heated_chamber: Self::default_heated_chamber(),
            printer_vendor: String::new(),
            printer_model: String::new(),
            travel_speed_mm_min: Self::default_travel_speed_mm_min(),
            z_hop_mm: Self::default_z_hop_mm(),
            retract_mm: Self::default_retract_mm(),
            retract_before_travel_mm: Self::default_retract_before_travel_mm(),
            retract_restart_extra_mm: Self::default_retract_restart_extra_mm(),
            retract_on_layer_change: Self::default_retract_on_layer_change(),
            use_firmware_retraction: Self::default_use_firmware_retraction(),
            use_relative_e_distances: Self::default_use_relative_e_distances(),
            wipe: Self::default_wipe(),
            wipe_distance_mm: Self::default_wipe_distance_mm(),
            retract_before_wipe_percent: Self::default_retract_before_wipe_percent(),
            only_one_wall_top: Self::default_only_one_wall_top(),
            only_one_wall_first_layer: Self::default_only_one_wall_first_layer(),
            support_threshold_angle: Self::default_support_threshold_angle(),
            infill_overlap_percent: Self::default_infill_overlap_percent(),
            infill_perimeter_gap_mm: Self::default_infill_perimeter_gap_mm(),
            min_infill_extrusion_mm: Self::default_min_infill_extrusion_mm(),
            path_tolerance: Self::default_path_tolerance(),
            gcode_flavor: Self::default_gcode_flavor(),
            triggers: Self::default_triggers(),
            fan_configs: Self::default_fan_configs(),
            mesh_quality: Self::default_mesh_quality(),
            first_layer_height: Self::default_first_layer_height(),
            line_width: Self::default_line_width(),
            outer_wall_line_width: Self::default_role_line_width(),
            inner_wall_line_width: Self::default_role_line_width(),
            top_surface_line_width: Self::default_role_line_width(),
            sparse_infill_line_width: Self::default_role_line_width(),
            support_line_width: Self::default_role_line_width(),
            retract_speed_mm_min: Self::default_retract_speed_mm_min(),
            flow_ratio: Self::default_flow_ratio(),
            nozzle_temp_first_layer: Self::default_nozzle_temp_first_layer(),
            bed_temp_first_layer: Self::default_bed_temp_first_layer(),
            chamber_temp: Self::default_chamber_temp(),
            chamber_temp_first_layer: Self::default_chamber_temp_first_layer(),
            filament_type: String::new(),
            filament_name: String::new(),
            filament_color: String::new(),
            pressure_advance: Self::default_pressure_advance(),
            acceleration: Self::default_acceleration(),
            first_layer_acceleration: Self::default_first_layer_acceleration(),
            top_surface_acceleration: Self::default_top_surface_acceleration(),
            outer_wall_acceleration: Self::default_outer_wall_acceleration(),
            bridge_acceleration: Self::default_bridge_acceleration(),
            inner_wall_acceleration: Self::default_inner_wall_acceleration(),
            sparse_infill_acceleration: Self::default_sparse_infill_acceleration(),
            solid_infill_acceleration: Self::default_solid_infill_acceleration(),
            gap_fill_acceleration: Self::default_gap_fill_acceleration(),
            support_acceleration: Self::default_support_acceleration(),
            travel_acceleration: Self::default_travel_acceleration(),
            square_corner_velocity: Self::default_square_corner_velocity(),
            max_velocity: Self::default_max_velocity(),
            time_estimate_warmup_s: Self::default_time_estimate_warmup_s(),
            time_estimate_cooldown_s: Self::default_time_estimate_cooldown_s(),
            time_estimate_scale: Self::default_time_estimate_scale(),
            disable_fan_first_layers: Self::default_disable_fan_first_layers(),
            max_volumetric_speed: Self::default_max_volumetric_speed(),
            extruder_count: Self::default_extruder_count(),
            support_enabled: Self::default_support_enabled(),
            support_type: SupportType::default(),
            support_density: Self::default_support_density(),
            support_interface_layers: Self::default_support_interface_layers(),
            support_interface_density: Self::default_support_interface_density(),
            support_xy_distance_mm: Self::default_support_xy_distance_mm(),
            support_z_gap_layers: Self::default_support_z_gap_layers(),
            support_on_build_plate_only: Self::default_support_on_build_plate_only(),
            support_auto: Self::default_support_auto(),
            adhesion_type: AdhesionType::default(),
            brim_width: Self::default_brim_width(),
            brim_type: BrimType::default(),
            brim_separation: Self::default_brim_separation(),
            skirt_loops: Self::default_skirt_loops(),
            skirt_distance: Self::default_skirt_distance(),
            skirt_height: Self::default_skirt_height(),
            raft_layers: Self::default_raft_layers(),
            raft_air_gap: Self::default_raft_air_gap(),
            elephant_foot_compensation_mm: Self::default_elephant_foot_compensation_mm(),
            elephant_foot_layers: Self::default_elephant_foot_layers(),
            elephant_foot_min_contour_width_mm: Self::default_elephant_foot_min_contour_width_mm(),
            ironing_enabled: Self::default_ironing_enabled(),
            ironing_type: IroningType::default(),
            ironing_flow: Self::default_ironing_flow(),
            ironing_spacing: Self::default_ironing_spacing(),
            ironing_speed: Self::default_ironing_speed(),
            ironing_angle: Self::default_ironing_angle(),
            xy_size_compensation: Self::default_xy_size_compensation(),
            xy_hole_compensation: Self::default_xy_hole_compensation(),
            start_gcode: None,
            end_gcode: None,
            layer_gcode: None,
            start_filament_gcode: None,
            end_filament_gcode: None,
            thumbnail_enabled: Self::default_thumbnail_enabled(),
            thumbnail_size_px: Self::default_thumbnail_size_px(),
            thumbnail_view: ThumbnailView::default(),
            thumbnail_theme: ThumbnailTheme::default(),
            thumbnail_color_mode: ThumbnailColorMode::default(),
            thumbnail_custom_color: Self::default_thumbnail_custom_color(),
            thumbnail_png_base64: None,
            print_sequence: PrintSequence::default(),
            exclude_object: false,
            extruder_clearance_height_mm: Self::default_extruder_clearance_height(),
            extruder_clearance_radius_mm: Self::default_extruder_clearance_radius(),
            between_objects_gcode: None,
            bed_mesh_mode: BedMeshMode::default(),
            bed_mesh_profile_name: None,
            bed_mesh_adaptive: false,
        }
    }
}

impl SlicingParams {
    /// Does this configuration need the plate sliced **object by object**?
    ///
    /// Object identity survives slicing only when something downstream needs
    /// it: firmware object markers ([`Self::exclude_object`]) or sequential
    /// printing ([`Self::print_sequence`]). When neither is on, the plate is
    /// merged into one mesh and sliced exactly as it always was, so the default
    /// configuration produces byte-identical G-code.
    pub fn object_aware(&self) -> bool {
        self.exclude_object || self.print_sequence == PrintSequence::ByObject
    }

    /// Serialize these params into a stable string for the G-code content cache
    /// key, **excluding the ephemeral thumbnail image payload**
    /// (`thumbnail_png_base64`).
    ///
    /// The embedded PNG is captured fresh from the viewer on every slice, so its
    /// bytes vary between renders (GPU/driver/anti-alias resolve, or an entirely
    /// different client) even for an otherwise-identical scene. Hashing it into
    /// the cache key would turn a re-slice into a miss and defeat cross-client
    /// reuse — exactly the "must not bust the cache on every camera nudge"
    /// requirement from issue #106. The thumbnail *settings* (view / theme /
    /// size / colour) are deliberately retained, so the preview embedded in a
    /// cached file always matches the request that reused it (the capture is a
    /// fixed, deterministic viewpoint, so identical settings yield an
    /// equivalent image).
    ///
    /// See the "G-code result cache" contract in AGENTS.md.
    pub fn cache_fingerprint(&self) -> String {
        let value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        let Some(object) = value.as_object() else {
            return value.to_string();
        };
        // Rebuild the map instead of `Map::remove`: with serde_json's
        // preserve-order backing, removing a key that is not the last one
        // *swaps the last entry into its slot*, so the surviving fields would
        // be ordered differently depending on whether the thumbnail was
        // present — two identical requests, two different cache keys.
        let filtered: serde_json::Map<String, serde_json::Value> = object
            .iter()
            .filter(|(key, _)| key.as_str() != "thumbnail_png_base64")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        serde_json::Value::Object(filtered).to_string()
    }

    fn default_first_layer_height() -> f64 {
        0.0
    }
    /// Typical FFF gantry clearance — a 25 mm tall part passes under most
    /// X-carriages. Matches PrusaSlicer's `extruder_clearance_height` default.
    fn default_extruder_clearance_height() -> f64 {
        25.0
    }
    /// Radius swept by the hotend and its fan duct. Matches PrusaSlicer's
    /// `extruder_clearance_radius` default.
    fn default_extruder_clearance_radius() -> f64 {
        45.0
    }

    fn default_line_width() -> f64 {
        0.0
    }
    /// Per-role line-width fields default to `0.0`, meaning "derive from the
    /// generic `line_width` / nozzle diameter". Any positive value is an
    /// explicit override for that extrusion role.
    fn default_role_line_width() -> f64 {
        0.0
    }
    fn default_retract_speed_mm_min() -> f64 {
        2400.0
    }
    fn default_flow_ratio() -> f64 {
        1.0
    }
    fn default_nozzle_temp_first_layer() -> f64 {
        0.0
    }
    fn default_bed_temp_first_layer() -> f64 {
        0.0
    }
    fn default_chamber_temp() -> f64 {
        0.0
    }
    fn default_chamber_temp_first_layer() -> f64 {
        0.0
    }
    fn default_heated_chamber() -> bool {
        false
    }
    fn default_pressure_advance() -> f64 {
        0.0
    }
    // Acceleration defaults target a modern, well-tuned CoreXY (input-shaped,
    // linear-rail class hardware capable of ~700 mm/s / 8000-12000 mm/s²), not
    // the lowest common denominator. A profile that never touches these gets a
    // fully populated, sensible acceleration scheme out of the box instead of
    // silent firmware defaults; a slower or untuned printer just prints these
    // values slower/rougher than intended and should dial them down (or zero
    // them to fall back to firmware behaviour) rather than the slicer guessing
    // conservative numbers that would waste a capable machine's headroom.
    fn default_acceleration() -> f64 {
        10000.0
    }
    fn default_first_layer_acceleration() -> f64 {
        // Adhesion is a squish/dwell-time constraint, not a speed-capability
        // one — kept low regardless of how fast the rest of the machine is.
        1000.0
    }
    fn default_top_surface_acceleration() -> f64 {
        6000.0
    }
    fn default_outer_wall_acceleration() -> f64 {
        // Visible-surface quality (ringing/ghosting) caps this well below the
        // machine's raw capability even on printers with input shaping.
        4000.0
    }
    fn default_bridge_acceleration() -> f64 {
        // Steady flow over unsupported spans matters more than machine
        // capability here — kept low like `first_layer_acceleration`.
        1000.0
    }
    fn default_inner_wall_acceleration() -> f64 {
        8000.0
    }
    fn default_sparse_infill_acceleration() -> f64 {
        // Interior and invisible: the highest printing acceleration.
        12000.0
    }
    fn default_solid_infill_acceleration() -> f64 {
        8000.0
    }
    fn default_gap_fill_acceleration() -> f64 {
        2000.0
    }
    fn default_support_acceleration() -> f64 {
        8000.0
    }
    fn default_travel_acceleration() -> f64 {
        // No material is deposited in transit, so travel gets the highest
        // acceleration of all — above even sparse infill.
        15000.0
    }
    fn default_square_corner_velocity() -> f64 {
        // `0` = defer to the estimator/firmware default (5 mm/s). Kept at 0 so a
        // profile that never set it doesn't start emitting an `M205`/velocity
        // limit that changes existing output.
        0.0
    }
    fn default_max_velocity() -> f64 {
        // `0` = no cap; role feedrates stand as emitted.
        0.0
    }
    fn default_time_estimate_warmup_s() -> f64 {
        0.0
    }
    fn default_time_estimate_cooldown_s() -> f64 {
        0.0
    }
    fn default_time_estimate_scale() -> f64 {
        1.0
    }
    fn default_disable_fan_first_layers() -> usize {
        1
    }
    fn default_max_volumetric_speed() -> f64 {
        0.0
    }
    fn default_extruder_count() -> usize {
        1
    }
    fn default_support_enabled() -> bool {
        false
    }
    fn default_support_auto() -> bool {
        true
    }
    fn default_support_density() -> f64 {
        0.15
    }
    fn default_support_interface_layers() -> usize {
        2
    }
    fn default_support_interface_density() -> f64 {
        0.7
    }
    /// Matches OrcaSlicer and Bambu Studio (0.35 mm); PrusaSlicer's percentage
    /// default lands near 0.22 mm. The 0.8 mm this used to be was chosen for
    /// easy teardown, but it reads as a visible moat around the part and lets
    /// steep overhangs sag before they reach the column holding them up.
    fn default_support_xy_distance_mm() -> f64 {
        0.35
    }
    fn default_support_z_gap_layers() -> usize {
        1
    }
    /// Supports may rest on the model by default, matching every mainstream
    /// slicer; build-plate-only is the opt-in that trades coverage for a
    /// clean, reachable teardown.
    fn default_support_on_build_plate_only() -> bool {
        false
    }
    fn default_brim_width() -> f64 {
        5.0
    }
    fn default_brim_separation() -> f64 {
        0.0
    }
    fn default_skirt_loops() -> usize {
        1
    }
    fn default_skirt_distance() -> f64 {
        2.0
    }
    fn default_skirt_height() -> usize {
        1
    }
    fn default_raft_layers() -> usize {
        0
    }
    fn default_raft_air_gap() -> f64 {
        0.1
    }
    /// The first layer is pressed into the bed and spreads, so a part comes out
    /// slightly too wide at the bottom and will not sit flush or fit a socket.
    /// 0.2 mm is the usual correction; it only ever shrinks the bottom layer.
    fn default_elephant_foot_compensation_mm() -> f64 {
        0.2
    }
    fn default_elephant_foot_layers() -> usize {
        1
    }
    fn default_elephant_foot_min_contour_width_mm() -> f64 {
        0.0
    }
    fn default_ironing_enabled() -> bool {
        false
    }
    /// 10 % of a solid bead — enough to re-melt the surface, not enough to
    /// raise it. Matches the PrusaSlicer/Orca default.
    fn default_ironing_flow() -> f64 {
        10.0
    }
    /// 0.1 mm, well under a bead width, so each pass overlaps its neighbour and
    /// the ridges between fill lines are spread rather than traced.
    fn default_ironing_spacing() -> f64 {
        0.1
    }
    fn default_ironing_speed() -> f64 {
        20.0
    }
    /// `-1` = follow the layer's own surface fill angle.
    fn default_ironing_angle() -> f64 {
        -1.0
    }
    /// Off. Dimensional compensation corrects a *specific* machine's error, so
    /// there is no useful non-zero default — and a non-zero one would silently
    /// resize every part the engine slices.
    fn default_xy_size_compensation() -> f64 {
        0.0
    }
    fn default_xy_hole_compensation() -> f64 {
        0.0
    }
    fn default_thumbnail_enabled() -> bool {
        true
    }
    fn default_thumbnail_size_px() -> u32 {
        320
    }
    fn default_thumbnail_custom_color() -> String {
        "#e0912f".to_string()
    }
}

impl SlicingParams {
    /// Human-readable warnings for settings that will **not** take effect.
    ///
    /// This is the "document + dummy logic" seam: rather than silently dropping
    /// a setting the user enabled, the slice path surfaces a warning so intent
    /// is visible. It covers two kinds of gap —
    ///
    /// 1. **Not implemented yet** — the feature is in the parameter set but not
    ///    in the pipeline. Each corresponds to a `TODO(profiles): …` marker at
    ///    the (future) implementation site.
    /// 2. **Unmet dependency** — the feature exists, but another setting it
    ///    needs is not configured. Typically cross-contract: the filament asks
    ///    for something the printer must provide. The UI shows these next to the
    ///    offending control with a link to the fix (see the field-exceptions
    ///    registry); this is the same honesty for every other front end.
    ///
    /// Implementation checklist (remove the branch here when each lands):
    /// - `TODO(profiles): multimaterial` — more than one extruder.
    pub fn unsupported_feature_warnings(&self) -> Vec<String> {
        let mut w = Vec::new();
        if self.support_enabled && self.support_type == SupportType::Tree {
            w.push(
                "tree supports drop and merge contact tips into branches, but do not yet flare \
into a wide base or route around obstacles"
                    .into(),
            );
        }
        if self.extruder_count > 1 {
            w.push("multiple extruders configured but multi-material slicing is not yet implemented — using tool 0".into());
        }
        // A chamber target without the machine capability emits nothing at all,
        // and a chamber that never heats looks exactly like one that does until
        // the part warps. `heated_chamber` is deliberately required (an unknown
        // chamber command aborts the print on Klipper), so say why and where.
        if !self.heated_chamber && self.chamber_temp_first_layer_resolved() > 0.0 {
            w.push(format!(
                "chamber temperature of {:.0} °C is set but the printer profile does not enable \
                 `heated_chamber` — no chamber command will be emitted; enable it on the printer \
                 if the machine has a chamber heater",
                self.chamber_temp_first_layer_resolved()
            ));
        }
        w
    }
}

impl SlicingParams {
    /// Return a copy of these parameters with the settings that are
    /// incompatible with spiral (vase) mode forced off, so the slicing pipeline
    /// and the G-code generator always observe a consistent single-wall
    /// configuration.
    ///
    /// When [`SlicingParams::spiral_vase`] is `false` this borrows `self`
    /// unchanged. When it is `true` it forces:
    /// - `wall_count = 1` (a single continuous perimeter),
    /// - `infill_density = 0` and `top_layers = 0` (nothing fills the hollow
    ///   interior of the vase),
    /// - `retract_mm = 0` and `z_hop_mm = 0` (the spiral is one uninterrupted
    ///   extrusion, so retraction/Z-hop would only stutter it),
    /// - `ironing_enabled = false`,
    /// - `support_enabled = false`.
    ///
    /// Support is forced off for the same reason as the rest: a vase is a
    /// single continuous wall climbing through Z, so there is no discrete layer
    /// for a support column to sit on and no retraction to lift the nozzle over
    /// one. Left on, the pipeline happily emitted support strands into a print
    /// that cannot travel between them.
    ///
    /// `bottom_layers` is intentionally left untouched: those solid layers form
    /// the vase's base. A user who wants an open-bottomed tube sets
    /// `bottom_layers = 0` explicitly.
    ///
    /// The method is idempotent, so it is safe to call at more than one pipeline
    /// boundary (it is applied both before slicing and before G-code emission).
    pub fn spiral_vase_normalized(&self) -> std::borrow::Cow<'_, SlicingParams> {
        if !self.spiral_vase {
            return std::borrow::Cow::Borrowed(self);
        }
        let mut p = self.clone();
        p.wall_count = 1;
        p.infill_density = 0.0;
        p.top_layers = 0;
        p.retract_mm = 0.0;
        p.z_hop_mm = 0.0;
        p.ironing_enabled = false;
        p.support_enabled = false;
        std::borrow::Cow::Owned(p)
    }
}

/// Thermal management — chamber targets and the part-cooling fan policy.
///
/// These resolve the "`0` = inherit" sentinels and the precedence between the
/// filament-owned cooling scalars and the `fan_configs` adaptive table, so the
/// G-code generator never re-derives the rules and they stay unit-testable.
impl SlicingParams {
    /// Chamber target for the first layer: [`SlicingParams::chamber_temp_first_layer`]
    /// when set, otherwise [`SlicingParams::chamber_temp`].
    pub fn chamber_temp_first_layer_resolved(&self) -> f64 {
        if self.chamber_temp_first_layer > 0.0 {
            self.chamber_temp_first_layer
        } else {
            self.chamber_temp
        }
    }

    /// Whether the slicer should emit real chamber heat directives.
    ///
    /// Requires the machine to declare [`SlicingParams::heated_chamber`] *and* a
    /// target above ambient — a chamber temperature of `0` means "don't manage
    /// the chamber", not "cool it down".
    pub fn chamber_heating_active(&self) -> bool {
        self.heated_chamber
            && (self.chamber_temp > 0.0 || self.chamber_temp_first_layer_resolved() > 0.0)
    }

    /// Whether the part-cooling fan is **pinned** to
    /// [`SlicingParams::first_layer_fan_speed`] on the given 0-based layer.
    ///
    /// True for the bottom [`SlicingParams::disable_fan_first_layers`] layers,
    /// where adhesion beats cooling. While pinned, the per-segment bridge and
    /// overhang fan overrides are suppressed too — otherwise a single overhang
    /// on layer 1 would defeat the whole point.
    pub fn part_cooling_pinned(&self, layer_index: usize) -> bool {
        layer_index < self.disable_fan_first_layers
    }

    /// Apply the filament-owned part-cooling policy on top of an adaptive speed
    /// computed from the `fan_configs` table.
    ///
    /// Precedence:
    /// 1. bottom `disable_fan_first_layers` layers → `first_layer_fan_speed`
    ///    (default `0.0`, i.e. fan off);
    /// 2. otherwise the adaptive speed, capped at `fan_speed` — the material's
    ///    cooling ceiling, which is what keeps ABS/ASA/PC from being blasted at
    ///    100 % while the chamber is trying to hold temperature.
    ///
    /// Applies to the part-cooling fan (`fan_index` 0) only; hotend, chamber and
    /// auxiliary fans keep their pure `fan_configs` + [`AuxFanOverrides`]
    /// behaviour.
    pub fn part_cooling_speed(&self, layer_index: usize, adaptive_speed: f64) -> f64 {
        if self.part_cooling_pinned(layer_index) {
            return self.first_layer_fan_speed.clamp(0.0, 1.0);
        }
        adaptive_speed
            .clamp(0.0, 1.0)
            .min(self.fan_speed.clamp(0.0, 1.0))
    }
}

impl SlicingParams {
    fn default_wall_generator() -> WallGenerator {
        WallGenerator::Arachne
    }

    fn default_wall_count() -> usize {
        3
    }

    fn default_wall_line_width_min() -> f64 {
        0.85
    }

    fn default_wall_line_width_max() -> f64 {
        1.5
    }

    fn default_wall_transition_threshold() -> f64 {
        0.6
    }

    fn default_wall_transition_length() -> f64 {
        1.0
    }

    fn default_wall_distribution_count() -> usize {
        1
    }

    fn default_wall_transition_angle() -> f64 {
        10.0
    }

    fn default_wall_transition_filter_distance() -> f64 {
        0.1
    }

    /// A seam that lands in the same place on every layer reads as one tidy
    /// line; `Nearest` scatters a blob at whatever vertex the nozzle happened to
    /// finish beside, which is what most of a first print's surface defects look
    /// like. The travel it costs is a fraction of a percent.
    fn default_seam_position() -> SeamPosition {
        SeamPosition::Aligned
    }

    fn default_external_perimeters_first() -> bool {
        false
    }

    fn default_extra_perimeters() -> bool {
        false
    }

    fn default_extra_perimeters_max_gap() -> f64 {
        3.0
    }

    fn default_thin_walls() -> bool {
        true
    }

    fn default_ensure_vertical_shell_thickness() -> bool {
        false
    }

    fn default_avoid_crossing_perimeters() -> bool {
        false
    }

    fn default_spiral_vase() -> bool {
        false
    }

    fn default_fuzzy_skin() -> bool {
        false
    }

    fn default_fuzzy_skin_thickness_mm() -> f64 {
        0.15
    }

    fn default_fuzzy_skin_point_dist_mm() -> f64 {
        0.5
    }

    fn default_infill_pattern() -> InfillPattern {
        InfillPattern::Rectilinear
    }

    fn default_infill_base_angle() -> f64 {
        45.0
    }

    fn default_infill_anchor_percent() -> f64 {
        // OrcaSlicer's default: 400 % of the sparse-infill line spacing.
        400.0
    }

    fn default_infill_anchor_max_mm() -> f64 {
        // OrcaSlicer's default cap on a wall stretch used to join two lines.
        20.0
    }

    fn default_infill_every_layers() -> u32 {
        1
    }

    fn default_infill_combination_max_layer_height_mm() -> f64 {
        // 0 = fall back to the nozzle diameter, the practical ceiling for how
        // tall a single bead can be laid.
        0.0
    }

    fn default_solid_infill_every_layers() -> u32 {
        0
    }

    /// The print speeds are one set, chosen for the same machine the
    /// acceleration defaults above assume, and they are the speeds the shipped
    /// process profile asks for — so a slice from the command line and a slice
    /// from the app produce the same print. Walls run slower than infill because
    /// they are the surface anyone looks at.
    fn default_perimeter_speed() -> f64 {
        80.0
    }

    fn default_infill_speed() -> f64 {
        150.0
    }

    fn default_bridge_speed() -> f64 {
        // Community-standard "smooth unsupported bridge" recipe: crawl the strand
        // across the gap so full part-cooling (`bridge_fan_speed`) freezes it in
        // place before it can sag, paired with the >1 `bridge_flow_ratio` that
        // fuses adjacent strands into a solid floor. 10 mm/s at 0.6 mm × 0.2 mm is
        // ≈ 1.2 mm³/s — well inside every filament's melt rate.
        10.0
    }

    fn default_enable_overhang_speed() -> bool {
        // On by default: the steep bands are *already* split off as
        // `OverhangPerimeter` by the binary classifier, so grading them costs no
        // extra path fragments — it only lets the near-airborne band print
        // slower and cooler than a half-supported one.
        true
    }

    /// Default for the three milder `overhang_*_speed` bands: `0` = inherit
    /// (Deg1/Deg2 → `perimeter_speed`, Deg3 → `bridge_speed`).
    ///
    /// Deg1/Deg2 centrelines lie *within* the previous layer's bead envelope
    /// (≥ 50 % supported) — the same "slight lean" geometry the overhang
    /// classifier deliberately refuses to flag. Slowing them would tax a large
    /// share of ordinary walls on any curved model for no quality gain, and
    /// would fragment those loops into arcs that print identically. Deg3
    /// inherits `bridge_speed`, so it tracks that setting instead of pinning a
    /// second number that can drift out of sync with it.
    fn default_overhang_degree_speed() -> f64 {
        0.0
    }

    /// Deg4 (75–100 % unsupported) in mm/s.
    ///
    /// This band is effectively extruding into air, but unlike a bridge it has
    /// no anchored far end to tension against — so it must stay the *slowest* of
    /// all, below `bridge_speed`. With the smooth-bridge default putting
    /// `bridge_speed` (which Deg3 inherits) at 10 mm/s, 8 mm/s keeps the grade
    /// strictly monotonic (perimeter → Deg3 → Deg4) instead of letting the most
    /// airborne band outrun the merely-steep one.
    fn default_overhang_4_4_speed() -> f64 {
        8.0
    }

    fn default_slowdown_for_curled_perimeters() -> bool {
        // Opt-in: this clamps every steep band to the *slowest* overhang speed,
        // which collapses Deg3 into Deg4 and discards the grading. Useful as a
        // deliberate curl-mitigation, wrong as a default.
        false
    }

    fn default_overhang_fan_speed() -> f64 {
        // Matches `bridge_fan_speed`: material laid over air is the case part
        // cooling exists for. Gated by `overhang_fan_threshold` so it only fires
        // on the steep bands.
        1.0
    }

    fn default_overhang_fan_threshold() -> f64 {
        // 50 % unsupported → Deg3/Deg4 only, aligning the fan boost with the
        // binary OverhangPerimeter classification. Keeping Deg1/Deg2 below the
        // threshold is also what lets them fold away instead of splitting walls.
        0.5
    }

    fn default_bridge_flow_ratio() -> f64 {
        // Deliberately over 1.0. Bridge lines are laid one nozzle-diameter apart,
        // so a 1.5× bead (0.6 mm at a 0.4 mm nozzle) overlaps its neighbours ~0.2 mm
        // and fuses them into a continuous, smooth floor instead of thin, gappy,
        // sag-prone strands. Paired with the slow `bridge_speed` and full
        // `bridge_fan_speed`, this is the "smooth unsupported bridge" recipe.
        1.5
    }

    fn default_bridge_min_area_mm2() -> f64 {
        0.5
    }

    fn default_bridge_noise_filter_mm() -> f64 {
        0.05
    }

    fn default_bridge_anchor_mm() -> f64 {
        // Anchor depth = 1 × nozzle diameter.  This inflates the bridge void
        // outward until the bridge lines start at the wall-bead inner edge
        // (which sits nozzle_diameter/2 from the void boundary, + another
        // nozzle_diameter/2 inside = nozzle_diameter total).  Smaller values
        // produce strands that barely touch the wall and sag; larger values
        // overlap the wall extrusions without the complementary wall-clipping
        // step.  0.4 mm is the typical nozzle diameter; callers may override
        // via `bridge_anchor_mm` in `SlicingParams`.
        0.4
    }

    fn default_top_surface_speed() -> f64 {
        60.0
    }

    fn default_gap_fill_speed() -> f64 {
        0.0
    }

    /// `0` = inherit `print_speed`. Support has no sensible absolute default:
    /// what is safe depends on how tall and thin the columns are, so the
    /// conservative choice is to print it like the rest of the model until a
    /// profile says otherwise.
    fn default_support_speed() -> f64 {
        0.0
    }

    fn default_gap_fill_min_length_mm() -> f64 {
        0.0
    }

    fn default_wall_overlap_compensation() -> f64 {
        0.0
    }

    /// Slow, and deliberately not scaled with the rest: the first layer is an
    /// adhesion problem, not a speed one.
    fn default_first_layer_speed() -> f64 {
        30.0
    }

    fn default_fan_speed() -> f64 {
        1.0
    }

    fn default_bridge_fan_speed() -> f64 {
        1.0
    }

    fn default_first_layer_fan_speed() -> f64 {
        0.0
    }

    /// Small layers otherwise land on plastic that is still molten, and the
    /// tip of a print slumps into a blob — the defect a first print is most
    /// likely to show. Filament profiles lower it for the materials that dislike
    /// being held back.
    fn default_min_layer_time_s() -> f64 {
        4.0
    }

    fn default_min_print_speed() -> f64 {
        10.0
    }

    /// Off. Coasting drops the last stretch of a path to an unfed nozzle to
    /// bleed off pressure; pressure advance does the same thing without leaving
    /// the end of every path under-extruded, and the shipped filament profiles
    /// set it.
    fn default_coasting_distance_mm() -> f64 {
        0.0
    }

    /// Four layers is 0.8 mm of top shell at the default layer height — the
    /// thickness the ecosystem settled on for a top surface that does not dimple
    /// over sparse infill. The bottom sits on the bed and needs one fewer.
    fn default_top_layers() -> usize {
        4
    }

    fn default_bottom_layers() -> usize {
        3
    }

    fn default_surface_infill_angle() -> f64 {
        45.0
    }

    fn default_top_surface_pattern() -> SurfacePattern {
        // OrcaSlicer's default: the most uniform-looking visible surface.
        SurfacePattern::MonotonicLine
    }

    fn default_bottom_surface_pattern() -> SurfacePattern {
        SurfacePattern::Monotonic
    }

    fn default_internal_solid_infill_pattern() -> SurfacePattern {
        SurfacePattern::Monotonic
    }

    fn default_bridge_angle() -> f64 {
        // 0 = detect automatically (PrusaSlicer/Orca convention).
        0.0
    }

    fn default_filament_diameter_mm() -> f64 {
        1.75
    }

    fn default_filament_density_g_cm3() -> f64 {
        // PLA — the most common FFF material.
        1.24
    }

    fn default_nozzle_diameter_mm() -> f64 {
        0.4
    }

    /// No Z compensation: emitted Z coordinates are the slice layer heights.
    fn default_z_offset_mm() -> f64 {
        0.0
    }

    fn default_travel_speed_mm_min() -> f64 {
        9000.0
    }

    fn default_z_hop_mm() -> f64 {
        0.2
    }

    fn default_retract_mm() -> f64 {
        1.0
    }

    fn default_retract_before_travel_mm() -> f64 {
        1.0
    }

    fn default_retract_restart_extra_mm() -> f64 {
        0.0
    }

    fn default_retract_on_layer_change() -> bool {
        false
    }

    fn default_use_firmware_retraction() -> bool {
        false
    }

    fn default_use_relative_e_distances() -> bool {
        false
    }

    /// Wiping while retracting costs nothing measurable and takes the bead of
    /// molten filament off the nozzle before it travels, so it does not land on
    /// the model as a string.
    fn default_wipe() -> bool {
        true
    }

    fn default_wipe_distance_mm() -> f64 {
        1.0
    }

    fn default_retract_before_wipe_percent() -> f64 {
        0.0
    }

    fn default_only_one_wall_top() -> bool {
        true // Single wall on top surface layers for cleaner finish
    }

    fn default_only_one_wall_first_layer() -> bool {
        true // Single wall on first layer for better bed adhesion
    }

    /// The classic 45° rule, measured from vertical: a wall may step outward by
    /// one layer height per layer and still rest on the layer below. Anything
    /// steeper overhangs air and gets support.
    fn default_support_threshold_angle() -> f64 {
        45.0
    }

    fn default_infill_overlap_percent() -> f64 {
        0.25 // 25% overlap for good bonding
    }

    fn default_infill_perimeter_gap_mm() -> f64 {
        0.1 // 0.1 mm gap between wall and sparse infill
    }

    fn default_min_infill_extrusion_mm() -> f64 {
        0.4 // one nozzle diameter — matches standard 0.4 mm nozzle default
    }

    fn default_path_tolerance() -> f64 {
        0.05
    }

    fn default_gcode_flavor() -> GcodeFlavor {
        GcodeFlavor::Marlin
    }

    fn default_triggers() -> Vec<PauseTrigger> {
        Vec::new()
    }

    fn default_fan_configs() -> Vec<FanConfig> {
        vec![FanConfig::default_part_cooling()]
    }

    fn default_mesh_quality() -> MeshQuality {
        MeshQuality::Normal
    }
}

/// Per-flavor lifecycle marker configuration.
///
/// Controls whether lifecycle markers are emitted in G-code output and allows
/// overriding the default marker strings for each supported annotation.
///
/// Template placeholders supported by marker override strings:
/// - `{z}` → current layer Z coordinate (e.g. `0.200`)
/// - `{height}` → layer height (e.g. `0.200`)
/// - `{type}` → extrusion role type name (e.g. `Perimeter`)
/// - `{width}` → default extrusion width for the role (e.g. `0.40`)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct LifecycleMarkerConfig {
    /// Whether to emit lifecycle markers at all. Default: true.
    #[serde(default = "LifecycleMarkerConfig::default_enabled")]
    pub enabled: bool,
    /// Override for `;LAYER_CHANGE`. Supports `{z}` and `{height}` placeholders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_change: Option<String>,
    /// Override for `;Z:{z}`. Supports `{z}` placeholder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z_marker: Option<String>,
    /// Override for `;HEIGHT:{height}`. Supports `{height}` placeholder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_marker: Option<String>,
    /// Override for `;BEFORE_LAYER_CHANGE`. Supports `{z}` placeholder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_layer_change: Option<String>,
    /// Override for `;AFTER_LAYER_CHANGE`. Supports `{z}` placeholder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_layer_change: Option<String>,
    /// Override for `;TYPE:{type}`. Supports `{type}` placeholder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_annotation: Option<String>,
    /// Override for `;WIDTH:{width}mm`. Supports `{width}` placeholder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_annotation: Option<String>,
}

impl Default for LifecycleMarkerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            layer_change: None,
            z_marker: None,
            height_marker: None,
            before_layer_change: None,
            after_layer_change: None,
            type_annotation: None,
            width_annotation: None,
        }
    }
}

impl LifecycleMarkerConfig {
    fn default_enabled() -> bool {
        true
    }
}

/// Per-object settings that may selectively override the global defaults.
///
/// `overrides` is `None` when no object-level customisation is applied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectSettings {
    /// Name of the object this settings block applies to.
    pub object_name: String,
    /// Optional parameter overrides for this object.
    /// `None` means the global settings apply without modification.
    pub overrides: Option<SlicingParams>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chamber_target_without_a_heated_chamber_is_reported() {
        // The filament asks for a chamber; the printer never said it has one.
        // Silence here is the failure mode — the print warps and nothing said why.
        let params = SlicingParams {
            heated_chamber: false,
            chamber_temp: 50.0,
            ..SlicingParams::default()
        };
        let warnings = params.unsupported_feature_warnings();
        assert!(
            warnings.iter().any(|w| w.contains("heated_chamber")),
            "expected a chamber warning, got: {warnings:?}"
        );
    }

    #[test]
    fn a_configured_chamber_is_not_warned_about() {
        let params = SlicingParams {
            heated_chamber: true,
            chamber_temp: 50.0,
            ..SlicingParams::default()
        };
        assert!(params
            .unsupported_feature_warnings()
            .iter()
            .all(|w| !w.contains("chamber")));
    }

    #[test]
    fn no_chamber_target_is_not_a_misconfiguration() {
        // The default (no chamber wanted, no heater) must stay silent — warning
        // about it would train users to ignore warnings.
        assert!(SlicingParams::default()
            .unsupported_feature_warnings()
            .iter()
            .all(|w| !w.contains("chamber")));
    }

    #[test]
    fn a_first_layer_only_chamber_target_is_still_reported() {
        let params = SlicingParams {
            heated_chamber: false,
            chamber_temp: 0.0,
            chamber_temp_first_layer: 60.0,
            ..SlicingParams::default()
        };
        assert!(params
            .unsupported_feature_warnings()
            .iter()
            .any(|w| w.contains("heated_chamber")));
    }

    /// Read a property's `x-relevant-when` gate out of the generated schema.
    fn relevance_gate(field: &str) -> Option<serde_json::Value> {
        let schema = schemars::schema_for!(SlicingParams);
        let json = serde_json::to_value(&schema).expect("schema to json");
        json.get("properties")?
            .get(field)?
            .get("x-relevant-when")
            .cloned()
    }

    #[test]
    fn generator_specific_wall_options_are_gated_in_the_schema() {
        // A wall option only one generator honours must be hidden for the other,
        // otherwise the UI offers a control that silently does nothing (or worse,
        // silently changes output — the caddy thin-wall regression).
        for (field, generator) in [
            ("thin_walls", "classic"),
            ("wall_distribution_count", "classic"),
            ("gap_fill_min_length_mm", "arachne"),
        ] {
            let gate = relevance_gate(field)
                .unwrap_or_else(|| panic!("{field} should carry an x-relevant-when gate"));
            assert_eq!(
                gate,
                serde_json::json!({ "field": "wall_generator", "equals": generator }),
                "{field} should only be shown for the {generator} generator"
            );
        }
    }

    /// Every conditional field must sit **below** the control that reveals it,
    /// in the same accordion group.
    ///
    /// The settings panel renders each group in schema-property order, which is
    /// this struct's declaration order. A gated field declared above its gate
    /// therefore appears *above* the switch the user just pressed, shoving that
    /// switch down the panel under their cursor — which is how `support_line_width`
    /// made the supports toggle jump on every press. A gate in a different group
    /// is the same defect at a distance: a section the user is not looking at
    /// silently grows or shrinks.
    ///
    /// Declaration order is invisible while reading a 4000-line struct, so it
    /// needs a test rather than a convention.
    #[test]
    fn every_conditional_field_follows_the_control_that_reveals_it() {
        let schema =
            serde_json::to_value(schemars::schema_for!(SlicingParams)).expect("schema serializes");
        let props = schema["properties"].as_object().expect("properties");

        let position: std::collections::HashMap<&str, usize> = props
            .keys()
            .enumerate()
            .map(|(i, k)| (k.as_str(), i))
            .collect();
        let group = |field: &str| props[field]["x-group"].as_str();

        let mut problems = Vec::new();
        for (field, spec) in props {
            let Some(gate) = spec.get("x-relevant-when").and_then(|r| r.get("field")) else {
                continue;
            };
            let gate = gate.as_str().expect("gate names a field");
            if !props.contains_key(gate) {
                problems.push(format!(
                    "{field} is gated on '{gate}', which is not a parameter"
                ));
                continue;
            }
            if group(field) != group(gate) {
                problems.push(format!(
                    "{field} is in {:?} but its gate {gate} is in {:?} — toggling one group \
                     would reshape another",
                    group(field),
                    group(gate)
                ));
            } else if position[field.as_str()] < position[gate] {
                problems.push(format!(
                    "{field} is declared above its gate {gate}, so it appears above the control \
                     that reveals it and pushes that control down the panel"
                ));
            }
        }

        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn extra_perimeters_max_gap_is_gated_on_its_parent_toggle() {
        let gate = relevance_gate("extra_perimeters_max_gap")
            .expect("extra_perimeters_max_gap should carry an x-relevant-when gate");
        assert_eq!(
            gate,
            serde_json::json!({ "field": "extra_perimeters", "equals": true }),
            "the gap threshold is meaningless unless extra_perimeters is on"
        );
    }

    #[test]
    fn options_both_generators_honour_are_not_gated() {
        // Guard against over-gating: these are honoured by classic *and* arachne,
        // so hiding either would lose a working control.
        for field in [
            "external_perimeters_first",
            "extra_perimeters",
            "wall_count",
        ] {
            assert!(
                relevance_gate(field).is_none(),
                "{field} works in both generators and must stay visible"
            );
        }
    }

    #[test]
    fn test_cache_fingerprint_excludes_thumbnail_png_payload() {
        // Two requests that differ *only* in the captured thumbnail image must
        // share a cache fingerprint — the PNG is camera-derived and re-rendered
        // every slice, so hashing it would defeat the cache (issue #106).
        let with_a = SlicingParams {
            thumbnail_png_base64: Some("iVBORw0KGgoAAAA".to_string()),
            ..SlicingParams::default()
        };
        let with_b = SlicingParams {
            thumbnail_png_base64: Some("Zm9vYmFyYmF6".to_string()),
            ..SlicingParams::default()
        };
        let without = SlicingParams {
            thumbnail_png_base64: None,
            ..SlicingParams::default()
        };
        assert_eq!(
            with_a.cache_fingerprint(),
            with_b.cache_fingerprint(),
            "different thumbnail images must not change the fingerprint"
        );
        assert_eq!(
            with_a.cache_fingerprint(),
            without.cache_fingerprint(),
            "presence/absence of the thumbnail image must not change the fingerprint"
        );
    }

    #[test]
    fn test_cache_fingerprint_tracks_toolpath_and_thumbnail_settings() {
        let base = SlicingParams::default();

        // A toolpath-affecting setting must change the fingerprint.
        let taller_layers = SlicingParams {
            layer_height: base.layer_height + 0.05,
            ..SlicingParams::default()
        };
        assert_ne!(
            base.cache_fingerprint(),
            taller_layers.cache_fingerprint(),
            "layer height must be part of the cache fingerprint"
        );

        // Thumbnail *settings* stay in the key so a cached file's embedded
        // preview always matches the request that reused it.
        let bigger_thumb = SlicingParams {
            thumbnail_size_px: base.thumbnail_size_px + 64,
            ..SlicingParams::default()
        };
        assert_ne!(
            base.cache_fingerprint(),
            bigger_thumb.cache_fingerprint(),
            "thumbnail settings must remain part of the cache fingerprint"
        );

        // The fingerprint must never carry the raw image bytes.
        let with_png = SlicingParams {
            thumbnail_png_base64: Some("SHOULD_NOT_APPEAR".to_string()),
            ..SlicingParams::default()
        };
        assert!(
            !with_png.cache_fingerprint().contains("SHOULD_NOT_APPEAR"),
            "cache fingerprint must not embed the thumbnail image payload"
        );
    }

    #[test]
    fn test_cache_fingerprint_tracks_triggers() {
        let base = SlicingParams::default();
        let with_trigger = SlicingParams {
            triggers: vec![PauseTrigger {
                position: TriggerPosition::AtLayer { layer: 5 },
                action: TriggerAction::Pause,
            }],
            ..SlicingParams::default()
        };
        assert_ne!(
            base.cache_fingerprint(),
            with_trigger.cache_fingerprint(),
            "triggers change the emitted g-code and must be part of the cache fingerprint"
        );
    }

    #[test]
    fn test_object_settings_with_overrides_round_trip() {
        let os = ObjectSettings {
            object_name: "part_a".to_string(),
            overrides: Some(SlicingParams {
                layer_height: 0.1,
                ..SlicingParams::default()
            }),
        };
        let json = serde_json::to_string(&os).expect("serialize");
        let back: ObjectSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.object_name, "part_a");
        assert_eq!(back.overrides.unwrap().layer_height, 0.1);
    }

    #[test]
    fn test_object_settings_without_overrides_round_trip() {
        let os = ObjectSettings {
            object_name: "part_b".to_string(),
            overrides: None,
        };
        let json = serde_json::to_string(&os).expect("serialize");
        let back: ObjectSettings = serde_json::from_str(&json).expect("deserialize");
        assert!(back.overrides.is_none());
    }

    // ── LifecycleMarkerConfig tests ──────────────────────────────────────────

    #[test]
    fn test_lifecycle_marker_config_default_enabled() {
        let cfg = LifecycleMarkerConfig::default();
        assert!(cfg.enabled);
        assert!(cfg.layer_change.is_none());
        assert!(cfg.z_marker.is_none());
        assert!(cfg.height_marker.is_none());
        assert!(cfg.before_layer_change.is_none());
        assert!(cfg.after_layer_change.is_none());
        assert!(cfg.type_annotation.is_none());
        assert!(cfg.width_annotation.is_none());
    }

    #[test]
    fn test_lifecycle_marker_config_round_trip() {
        let cfg = LifecycleMarkerConfig {
            enabled: false,
            layer_change: Some("LAYER_CHANGE {z}".to_string()),
            z_marker: Some(";Z:{z}".to_string()),
            ..LifecycleMarkerConfig::default()
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: LifecycleMarkerConfig = serde_json::from_str(&json).expect("deserialize");
        assert!(!back.enabled);
        assert_eq!(back.layer_change.as_deref(), Some("LAYER_CHANGE {z}"));
        assert_eq!(back.z_marker.as_deref(), Some(";Z:{z}"));
    }

    #[test]
    fn test_lifecycle_marker_config_defaults_when_absent() {
        let json = r#"{}"#;
        let cfg: LifecycleMarkerConfig = serde_json::from_str(json).expect("deserialize");
        assert!(cfg.enabled, "enabled should default to true when absent");
    }

    #[test]
    fn test_lifecycle_marker_config_none_fields_omitted() {
        let cfg = LifecycleMarkerConfig::default();
        let json = serde_json::to_string(&cfg).expect("serialize");
        assert!(!json.contains("layer_change"), "None field omitted");
        assert!(!json.contains("z_marker"), "None field omitted");
    }

    #[test]
    fn test_slicing_params_top_bottom_layers_defaults() {
        let params = SlicingParams::default();
        assert_eq!(params.top_layers, 4, "Default top layers should be 4");
        assert_eq!(params.bottom_layers, 3, "Default bottom layers should be 3");
        assert_eq!(
            params.surface_infill_angle, 45.0,
            "Default surface infill angle should be 45°"
        );
    }

    #[test]
    fn test_perimeter_routing_defaults() {
        let p = SlicingParams::default();
        assert!(
            !p.external_perimeters_first,
            "outer wall prints last by default (ecosystem standard)"
        );
        assert!(!p.extra_perimeters, "extra_perimeters off by default");
        assert_eq!(p.extra_perimeters_max_gap, 3.0);
        assert!(p.thin_walls, "thin-wall gap fill on by default");
        assert!(
            !p.ensure_vertical_shell_thickness,
            "vertical-shell enforcement off by default"
        );
        assert!(
            !p.avoid_crossing_perimeters,
            "avoid_crossing_perimeters off by default"
        );
    }

    #[test]
    fn test_perimeter_routing_round_trips_through_json() {
        let p = SlicingParams {
            external_perimeters_first: true,
            extra_perimeters: true,
            extra_perimeters_max_gap: 2.5,
            thin_walls: false,
            ensure_vertical_shell_thickness: true,
            avoid_crossing_perimeters: true,
            ..SlicingParams::default()
        };
        let json = serde_json::to_string(&p).expect("serialize");
        let back: SlicingParams = serde_json::from_str(&json).expect("deserialize");
        assert!(back.external_perimeters_first);
        assert!(back.extra_perimeters);
        assert_eq!(back.extra_perimeters_max_gap, 2.5);
        assert!(!back.thin_walls);
        assert!(back.ensure_vertical_shell_thickness);
        assert!(back.avoid_crossing_perimeters);
    }

    #[test]
    fn test_perimeter_routing_absent_keys_use_defaults() {
        // A sparse process-profile params bag omits these keys → engine defaults.
        let p: SlicingParams = serde_json::from_str(r#"{"wall_count": 2}"#).expect("deserialize");
        assert!(!p.external_perimeters_first);
        assert!(p.thin_walls);
        assert!(!p.avoid_crossing_perimeters);
    }

    #[test]
    fn test_slicing_params_arachne_defaults() {
        let params = SlicingParams::default();
        assert_eq!(params.wall_count, 3, "Default wall count should be 3");
        assert_eq!(
            params.wall_line_width_min, 0.85,
            "Default wall_line_width_min should be 0.85"
        );
        assert_eq!(
            params.wall_line_width_max, 1.5,
            "Default wall_line_width_max should be 1.5"
        );
        assert_eq!(
            params.wall_transition_threshold, 0.6,
            "Default wall_transition_threshold should be 0.6"
        );
        assert_eq!(
            params.wall_transition_length, 1.0,
            "Default wall_transition_length should be 1.0"
        );
        assert_eq!(
            params.wall_distribution_count, 1,
            "Default wall_distribution_count should be 1"
        );
    }

    #[test]
    fn test_slicing_params_arachne_fields_round_trip() {
        let params = SlicingParams {
            wall_count: 5,
            wall_line_width_min: 0.6,
            wall_line_width_max: 2.0,
            wall_transition_threshold: 0.4,
            wall_transition_length: 0.8,
            wall_distribution_count: 2,
            ..SlicingParams::default()
        };
        let json = serde_json::to_string(&params).expect("serialize");
        let back: SlicingParams = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.wall_count, 5);
        assert_eq!(back.wall_line_width_min, 0.6);
        assert_eq!(back.wall_line_width_max, 2.0);
        assert_eq!(back.wall_transition_threshold, 0.4);
        assert_eq!(back.wall_transition_length, 0.8);
        assert_eq!(back.wall_distribution_count, 2);
    }

    #[test]
    fn test_slicing_params_top_bottom_layers_serialization() {
        let params = SlicingParams {
            top_layers: 5,
            bottom_layers: 4,
            surface_infill_angle: 60.0,
            ..SlicingParams::default()
        };
        let json = serde_json::to_string(&params).expect("serialize");
        let back: SlicingParams = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.top_layers, 5);
        assert_eq!(back.bottom_layers, 4);
        assert_eq!(back.surface_infill_angle, 60.0);
    }

    #[test]
    fn test_slicing_params_legacy_json_without_surface_layers() {
        // Test that old JSON without top_layers/bottom_layers/surface_infill_angle still deserializes.
        // Unknown fields such as "wall_thickness" from legacy files are silently ignored.
        let json = r#"{"layer_height":0.2,"infill_density":0.2,"print_speed":60.0,"nozzle_temp":210.0,"bed_temp":60.0}"#;
        let params: SlicingParams = serde_json::from_str(json).expect("deserialize");
        assert_eq!(params.top_layers, 4, "Should default to 4 for legacy JSON");
        assert_eq!(
            params.bottom_layers, 3,
            "Should default to 3 for legacy JSON"
        );
        assert_eq!(
            params.surface_infill_angle, 45.0,
            "Should default to 45.0 for legacy JSON"
        );
    }

    #[test]
    fn test_slicing_params_hardware_defaults() {
        let params = SlicingParams::default();
        assert_eq!(params.filament_diameter_mm, 1.75);
        assert_eq!(params.nozzle_diameter_mm, 0.4);
        assert_eq!(params.travel_speed_mm_min, 9000.0);
        assert_eq!(params.z_hop_mm, 0.2);
        assert_eq!(params.retract_mm, 1.0);
        assert_eq!(params.path_tolerance, 0.05);
        assert!(params.thumbnail_enabled);
        assert_eq!(params.thumbnail_size_px, 320);
        assert_eq!(params.thumbnail_view, ThumbnailView::Isometric);
        assert_eq!(params.thumbnail_theme, ThumbnailTheme::Transparent);
        assert_eq!(params.thumbnail_color_mode, ThumbnailColorMode::Filament);
        assert_eq!(params.thumbnail_custom_color, "#e0912f");
        assert!(params.thumbnail_png_base64.is_none());
    }

    #[test]
    fn test_slicing_params_hardware_fields_round_trip() {
        let params = SlicingParams {
            filament_diameter_mm: 2.85,
            nozzle_diameter_mm: 0.6,
            travel_speed_mm_min: 12000.0,
            z_hop_mm: 0.4,
            retract_mm: 2.0,
            ..SlicingParams::default()
        };
        let json = serde_json::to_string(&params).expect("serialize");
        let back: SlicingParams = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.filament_diameter_mm, 2.85);
        assert_eq!(back.nozzle_diameter_mm, 0.6);
        assert_eq!(back.travel_speed_mm_min, 12000.0);
        assert_eq!(back.z_hop_mm, 0.4);
        assert_eq!(back.retract_mm, 2.0);
    }

    #[test]
    fn test_slicing_params_hardware_fields_default_when_absent() {
        // Legacy JSON without the new fields should still deserialize with defaults
        let json = r#"{"layer_height":0.2,"infill_density":0.2,"print_speed":60.0,"nozzle_temp":210.0,"bed_temp":60.0}"#;
        let params: SlicingParams = serde_json::from_str(json).expect("deserialize");
        assert_eq!(
            params.filament_diameter_mm, 1.75,
            "default filament diameter"
        );
        assert_eq!(params.nozzle_diameter_mm, 0.4, "default nozzle diameter");
        assert_eq!(params.travel_speed_mm_min, 9000.0, "default travel speed");
        assert_eq!(params.z_hop_mm, 0.2, "default z-hop");
        assert_eq!(params.retract_mm, 1.0, "default retract");
        assert_eq!(params.path_tolerance, 0.05, "default path tolerance");
        assert!(params.thumbnail_enabled, "default thumbnail enabled");
        assert_eq!(params.thumbnail_size_px, 320, "default thumbnail size");
        assert_eq!(
            params.thumbnail_view,
            ThumbnailView::Isometric,
            "default thumbnail view"
        );
        assert_eq!(
            params.thumbnail_theme,
            ThumbnailTheme::Transparent,
            "default thumbnail theme"
        );
        assert_eq!(
            params.thumbnail_color_mode,
            ThumbnailColorMode::Filament,
            "default thumbnail colour mode"
        );
        assert_eq!(
            params.thumbnail_custom_color, "#e0912f",
            "default thumbnail custom colour"
        );
        assert!(
            params.thumbnail_png_base64.is_none(),
            "default thumbnail payload absent"
        );
    }
}
