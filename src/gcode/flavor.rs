//! [`GcodeFlavor`] enum — selects the firmware dialect at generator creation time.
//!
//! Also home to [`ForeignFlavor`]: the firmware names other slicers' presets
//! use, and which of our dialects serves each of them.

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
    #[serde(alias = "Marlin", alias = "marlin2")]
    Marlin,
    /// Adds velocity, pressure-advance and custom-macro commands.
    #[serde(alias = "Klipper")]
    Klipper,
    /// Duet and other RepRapFirmware boards, with their own pause, leveling and
    /// motion-limit commands.
    // `reprap` was this flavor's token before it was spelled out in full.
    #[serde(
        alias = "RepRapFirmware",
        alias = "reprap",
        alias = "RepRap",
        alias = "rrf"
    )]
    RepRapFirmware,
}

impl GcodeFlavor {
    /// Every flavor, in the order the UI lists them.
    pub const ALL: [GcodeFlavor; 3] = [Self::Marlin, Self::Klipper, Self::RepRapFirmware];

    /// The flavor that serves a firmware named by another slicer's preset.
    ///
    /// `token` is that preset's `gcode_flavor` value, matched without regard to
    /// case. `None` means no preset format we read uses the token.
    pub fn from_foreign(token: &str) -> Option<&'static ForeignFlavor> {
        let token = token.trim();
        FOREIGN_FLAVORS
            .iter()
            .find(|f| f.token.eq_ignore_ascii_case(token))
    }
}

impl FromStr for GcodeFlavor {
    type Err = String;

    /// Parse one of our own tokens.
    ///
    /// A firmware another slicer names but we only approximate is refused, with
    /// the nearest flavor and what it costs in the message: picking it is the
    /// caller's decision to make knowingly, not ours to make silently.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let token = s.trim().to_ascii_lowercase();
        match token.as_str() {
            "marlin" | "marlin2" => Ok(Self::Marlin),
            "klipper" => Ok(Self::Klipper),
            "reprapfirmware" | "rrf" | "reprap" => Ok(Self::RepRapFirmware),
            _ => Err(match Self::from_foreign(&token) {
                Some(foreign) => format!(
                    "{} has no G-code flavor of its own; '{}' is the nearest. {}",
                    foreign.firmware,
                    foreign.flavor,
                    foreign.caveat.unwrap_or_default()
                ),
                None => format!(
                    "Unknown G-code flavor '{}'. Supported: marlin, klipper, reprapfirmware",
                    s
                ),
            }),
        }
    }
}

impl std::fmt::Display for GcodeFlavor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Marlin => write!(f, "marlin"),
            Self::Klipper => write!(f, "klipper"),
            Self::RepRapFirmware => write!(f, "reprapfirmware"),
        }
    }
}

/// A firmware another slicer's preset names, and what we generate for it.
///
/// Those presets choose from a longer list of firmwares than we have dialects
/// for. Coercing one silently would hand the user G-code their printer reads
/// differently without telling them, so every entry says whether its dialect
/// was written for that firmware and, when it was not, what to expect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForeignFlavor {
    /// The `gcode_flavor` token as the preset spells it.
    pub token: &'static str,
    /// The firmware's name, as a person would say it.
    pub firmware: &'static str,
    /// The dialect we generate for it.
    pub flavor: GcodeFlavor,
    /// `None` when [`Self::flavor`] is written for this firmware; otherwise
    /// what the firmware does differently with the G-code we generate.
    pub caveat: Option<&'static str>,
}

impl ForeignFlavor {
    /// Whether our dialect was written for this firmware.
    pub fn is_exact(&self) -> bool {
        self.caveat.is_none()
    }
}

/// Every `gcode_flavor` token the preset formats we read can carry.
///
/// Note `reprap`: in those presets it names the old RepRap/Sprinter firmware —
/// Marlin's ancestor — not RepRapFirmware, which is `reprapfirmware`.
const FOREIGN_FLAVORS: &[ForeignFlavor] = &[
    ForeignFlavor {
        token: "marlin2",
        firmware: "Marlin 2",
        flavor: GcodeFlavor::Marlin,
        caveat: None,
    },
    ForeignFlavor {
        token: "marlin",
        firmware: "Marlin (legacy)",
        flavor: GcodeFlavor::Marlin,
        caveat: Some(
            "The G-code is written for Marlin 2. Older Marlin runs the moves, \
             temperatures and fans unchanged and ignores the commands it predates, \
             such as object labels and junction deviation.",
        ),
    },
    ForeignFlavor {
        token: "klipper",
        firmware: "Klipper",
        flavor: GcodeFlavor::Klipper,
        caveat: None,
    },
    ForeignFlavor {
        token: "reprapfirmware",
        firmware: "RepRapFirmware",
        flavor: GcodeFlavor::RepRapFirmware,
        caveat: None,
    },
    ForeignFlavor {
        token: "reprap",
        firmware: "RepRap/Sprinter",
        flavor: GcodeFlavor::Marlin,
        caveat: Some(
            "RepRap/Sprinter is the old name for Marlin-compatible firmware, so this \
             printer gets Marlin G-code. If it runs RepRapFirmware on a Duet board, \
             choose RepRapFirmware instead.",
        ),
    },
    ForeignFlavor {
        token: "repetier",
        firmware: "Repetier-Firmware",
        flavor: GcodeFlavor::Marlin,
        caveat: Some(
            "Repetier runs Marlin's moves, temperatures and fans but reads several \
             setup commands differently: acceleration control and pressure advance \
             have no effect, and firmware retraction setup would change its jerk \
             instead, so keep firmware retraction off.",
        ),
    },
    ForeignFlavor {
        token: "smoothie",
        firmware: "Smoothieware",
        flavor: GcodeFlavor::Marlin,
        caveat: Some(
            "Smoothieware runs Marlin's moves, temperatures, fans and firmware \
             retraction. It ignores Marlin's acceleration, cornering and pressure \
             advance commands, and levels the bed with different ones.",
        ),
    },
    ForeignFlavor {
        token: "teacup",
        firmware: "Teacup",
        flavor: GcodeFlavor::Marlin,
        caveat: Some(
            "Teacup runs the moves and temperatures; Marlin's setup commands for \
             acceleration, retraction and leveling are unknown to it.",
        ),
    },
    ForeignFlavor {
        token: "makerware",
        firmware: "MakerBot (MakerWare)",
        flavor: GcodeFlavor::Marlin,
        caveat: Some(
            "MakerBot printers read X3G, not G-code: convert the file (for example \
             with GPX) before printing. MakerBot-specific commands are not generated.",
        ),
    },
    ForeignFlavor {
        token: "sailfish",
        firmware: "Sailfish",
        flavor: GcodeFlavor::Marlin,
        caveat: Some(
            "Sailfish printers read X3G, not G-code: convert the file (for example \
             with GPX) before printing. Sailfish-specific commands are not generated.",
        ),
    },
    ForeignFlavor {
        token: "mach3",
        firmware: "Mach3",
        flavor: GcodeFlavor::Marlin,
        caveat: Some(
            "A Mach3 machine set up for printing drives the extruder as an A axis and \
             sets temperatures its own way; this output uses E and Marlin heater \
             commands, so it needs post-processing first.",
        ),
    },
    ForeignFlavor {
        token: "machinekit",
        firmware: "Machinekit / LinuxCNC",
        flavor: GcodeFlavor::Marlin,
        caveat: Some(
            "A LinuxCNC machine set up for printing drives the extruder as an A axis \
             and sets temperatures its own way; this output uses E and Marlin heater \
             commands, so it needs post-processing first.",
        ),
    },
    ForeignFlavor {
        token: "no-extrusion",
        firmware: "No extrusion",
        flavor: GcodeFlavor::Marlin,
        caveat: Some(
            "This machine was set up for G-code without extrusion, such as a plotter \
             or a test rig. The output always extrudes.",
        ),
    },
    ForeignFlavor {
        token: "bambu",
        firmware: "Bambu Lab",
        flavor: GcodeFlavor::Marlin,
        caveat: Some(
            "Bambu Lab printers accept plain Marlin-style moves, temperatures and \
             fans. Their own extensions — AMS filament changes, calibration and \
             timelapse — are not generated.",
        ),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_own_tokens_round_trip_through_display_and_from_str() {
        for flavor in GcodeFlavor::ALL {
            assert_eq!(flavor.to_string().parse::<GcodeFlavor>(), Ok(flavor));
        }
    }

    #[test]
    fn serde_spells_reprapfirmware_out_and_still_reads_the_old_token() {
        assert_eq!(
            serde_json::to_value(GcodeFlavor::RepRapFirmware).unwrap(),
            "reprapfirmware"
        );
        for old in ["\"reprap\"", "\"RepRap\"", "\"RepRapFirmware\""] {
            assert_eq!(
                serde_json::from_str::<GcodeFlavor>(old).unwrap(),
                GcodeFlavor::RepRapFirmware,
                "{old} must keep loading"
            );
        }
    }

    #[test]
    fn exact_foreign_tokens_parse_and_approximations_are_refused_with_the_nearest() {
        assert_eq!("marlin2".parse(), Ok(GcodeFlavor::Marlin));
        assert_eq!("RRF".parse(), Ok(GcodeFlavor::RepRapFirmware));

        let err = "smoothie".parse::<GcodeFlavor>().unwrap_err();
        assert!(err.contains("Smoothieware"), "{err}");
        assert!(err.contains("'marlin'"), "names the nearest flavor: {err}");
        assert!(err.contains("acceleration"), "carries the caveat: {err}");
    }

    #[test]
    fn every_foreign_token_is_unique_lowercase_and_explains_any_approximation() {
        for (i, foreign) in FOREIGN_FLAVORS.iter().enumerate() {
            assert_eq!(foreign.token, foreign.token.to_ascii_lowercase());
            assert!(
                !FOREIGN_FLAVORS[..i]
                    .iter()
                    .any(|f| f.token == foreign.token),
                "duplicate token {}",
                foreign.token
            );
            if let Some(caveat) = foreign.caveat {
                assert!(caveat.ends_with('.'), "{}: {caveat}", foreign.token);
            }
        }
    }

    /// The trap the table exists for: the same five letters name different
    /// firmware in a foreign preset and in our own settings.
    #[test]
    fn foreign_reprap_is_sprinter_not_reprapfirmware() {
        let sprinter = GcodeFlavor::from_foreign("reprap").unwrap();
        assert_eq!(sprinter.flavor, GcodeFlavor::Marlin);
        assert!(!sprinter.is_exact());

        let rrf = GcodeFlavor::from_foreign("RepRapFirmware").unwrap();
        assert_eq!(rrf.flavor, GcodeFlavor::RepRapFirmware);
        assert!(rrf.is_exact());
    }

    #[test]
    fn unknown_foreign_tokens_are_none() {
        assert!(GcodeFlavor::from_foreign("bogus").is_none());
        assert!(GcodeFlavor::from_foreign("").is_none());
    }
}
