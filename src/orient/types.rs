use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Options controlling the auto-orient algorithm.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct AutoOrientOptions {
    /// When `true`, additionally sample a Fibonacci-sphere grid (~128
    /// candidates) in addition to the flat-face candidates.  Recommended for
    /// organic shapes with no large flat faces, e.g. figurines.  When
    /// `false` (default), only unique flat-face-normal directions are tested —
    /// fast and correct for box-like objects.
    pub allow_rotations: bool,

    /// After finding the best face-down orientation, additionally rotate the
    /// object around Z by this many degrees.  Set to `45.0` for CoreXY
    /// printers to align the seam line with the stepper axes.  `0.0` =
    /// disabled (default).
    pub preferred_z_rotation_deg: f64,

    /// Faces whose outward normal points more than this many degrees below
    /// horizontal are counted as overhanging (and penalised).
    ///
    /// Same convention as
    /// [`SlicingParams::support_threshold_angle`](crate::settings::params::SlicingParams::support_threshold_angle)
    /// — a surface leaning `θ` from vertical puts its normal `θ` below
    /// horizontal — so the two numbers are directly comparable and want to
    /// agree: orienting against a threshold the support pass does not share
    /// means avoiding overhangs that would have been supported anyway.
    /// **Default: 45°.**
    pub overhang_threshold_deg: f64,
}

impl Default for AutoOrientOptions {
    fn default() -> Self {
        Self {
            allow_rotations: false,
            preferred_z_rotation_deg: 0.0,
            overhang_threshold_deg: 45.0,
        }
    }
}

/// Options controlling the multi-object `ArrangeOnBed` operation.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ArrangeOptions {
    /// Gap between adjacent objects in millimetres.  **Default: 2.0 mm.**
    pub spacing_mm: f64,

    /// When `true`, auto-orient every object before packing.
    /// Each object is oriented to minimise overhangs before its footprint
    /// is computed; the resulting outlines are then fed to the packer.
    /// **Default: true.**
    pub auto_orient: bool,

    /// Options forwarded to [`crate::orient::auto_orient`] when
    /// `auto_orient` is `true`.
    ///
    /// One field is read either way:
    /// [`AutoOrientOptions::overhang_threshold_deg`] also tells the packer
    /// which undersides hold themselves up, and so which parts may have
    /// something tucked beneath them.
    pub orient_options: AutoOrientOptions,

    /// May a part take plate area above or below another part?
    ///
    /// A plate printed a layer at a time never lowers the nozzle below what is
    /// already printed, so a part leaning at 45° may hang over its neighbour's
    /// foot and a short part may tuck in under a flared rim.  Set this to
    /// `false` for sequential printing, where the gantry drives past finished
    /// parts and every part needs its own column of air.  **Default: true.**
    pub vertical_nesting: bool,

    /// Rotation step, in degrees, the packer may turn an object by to make it
    /// fit.  `0.0` (or anything ≥ 360) keeps every object at the angle it
    /// arrived in.  **Default: 90°.**
    ///
    /// Ninety degrees is free: a quarter turn cannot undo an auto-orient
    /// result, and it preserves
    /// [`AutoOrientOptions::preferred_z_rotation_deg`] — a CoreXY machine
    /// asking for 45° still gets a diagonal part afterwards.  Finer steps nest
    /// tighter but turn parts off whatever angle the user chose.
    pub rotation_step_deg: f64,
}

impl Default for ArrangeOptions {
    fn default() -> Self {
        Self {
            spacing_mm: 2.0,
            auto_orient: true,
            orient_options: AutoOrientOptions::default(),
            vertical_nesting: true,
            rotation_step_deg: 90.0,
        }
    }
}
