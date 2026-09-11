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
    /// is computed; the result is then fed into the shelf-packing layout.
    /// **Default: true.**
    pub auto_orient: bool,

    /// Options forwarded to [`crate::orient::auto_orient`] when
    /// `auto_orient` is `true`.  Ignored otherwise.
    pub orient_options: AutoOrientOptions,
}

impl Default for ArrangeOptions {
    fn default() -> Self {
        Self {
            spacing_mm: 2.0,
            auto_orient: true,
            orient_options: AutoOrientOptions::default(),
        }
    }
}
