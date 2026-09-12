//! [`GcodeFlavor`] enum — selects the firmware dialect at generator creation time.

use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Supported G-code firmware flavors.
///
/// Each variant selects the concrete [`crate::gcode::GcodeDialect`] used by
/// [`crate::gcode::GcodeGenerator`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GcodeFlavor {
    /// The standard M-command set, understood by most consumer FDM printers.
    #[default]
    #[serde(alias = "Marlin")]
    Marlin,
    /// Adds `SET_VELOCITY_LIMIT`, `SET_PRESSURE_ADVANCE`, and custom macros.
    #[serde(alias = "Klipper")]
    Klipper,
    /// A Marlin-compatible baseline plus a few RepRapFirmware-only commands
    /// (e.g. `M226` for pause-on-this-line).
    #[serde(alias = "RepRap")]
    RepRap,
}

impl FromStr for GcodeFlavor {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "marlin" => Ok(Self::Marlin),
            "klipper" => Ok(Self::Klipper),
            "reprap" => Ok(Self::RepRap),
            _ => Err(format!(
                "Unknown G-code flavor '{}'. Supported: marlin, klipper, reprap",
                s
            )),
        }
    }
}

impl std::fmt::Display for GcodeFlavor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Marlin => write!(f, "marlin"),
            Self::Klipper => write!(f, "klipper"),
            Self::RepRap => write!(f, "reprap"),
        }
    }
}
