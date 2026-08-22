//! [`GcodeFlavor`] enum — selects the firmware dialect at generator creation time.

use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Supported G-code firmware flavors.
///
/// Each variant selects the concrete [`crate::gcode::GcodeDialect`] used by
/// [`crate::gcode::GcodeGenerator`].  Only **Marlin** and **Klipper** are
/// first-class citizens; additional flavors will be added in future releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GcodeFlavor {
    /// Marlin firmware: standard M-command set, widely compatible with consumer FDM printers.
    #[default]
    #[serde(alias = "Marlin")]
    Marlin,
    /// Klipper firmware: supports `SET_VELOCITY_LIMIT`, `SET_PRESSURE_ADVANCE`, and custom macros.
    #[serde(alias = "Klipper")]
    Klipper,
}

impl FromStr for GcodeFlavor {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "marlin" => Ok(Self::Marlin),
            "klipper" => Ok(Self::Klipper),
            _ => Err(format!(
                "Unknown G-code flavor '{}'. Supported: marlin, klipper",
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
        }
    }
}
