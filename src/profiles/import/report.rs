//! What an import produced, and what it did with every setting it read.
//!
//! The report is the point of the importer as much as the profiles are. Another
//! slicer's preset carries hundreds of settings, and many have no exact
//! counterpart here; a user who cannot see which ones came across unchanged,
//! which were worked out, which only approximate the original and which were
//! dropped has no way to trust the profile they end up with. So every setting
//! read from the file gets an [`ImportEntry`], and none is coerced or dropped
//! without one.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::profiles::{FilamentProfile, PrinterProfile, ProcessProfile};

/// How one setting from the file fared.
///
/// Ordered from "nothing to check" to "nothing kept", which is the order a
/// review lists them in.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ImportOutcome {
    /// Same meaning and value here, possibly under another name.
    Imported,
    /// Same meaning, but the value had to be worked out — a unit change, a
    /// percentage of another setting resolved, or a fallback the source applies.
    Converted,
    /// The nearest equivalent. Prints may come out differently.
    Approximated,
    /// A feature this slicer doesn't have, switched on in the file. Its effect
    /// is lost.
    Unsupported,
    /// Recognised, and makes no difference here: metadata, an option that is
    /// off, or something that only matters with hardware this profile lacks.
    Ignored,
    /// Not a setting this importer knows.
    Unknown,
}

/// One value an entry wrote into an imported profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImportedValue {
    /// The setting it went to: a `SlicingParams` key, or a profile field such
    /// as `bed_width`, `material` or `vendor`.
    pub setting: String,
    /// The value as stored in the profile.
    pub value: serde_json::Value,
}

/// What became of one setting read from the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImportEntry {
    /// The setting's name in the file.
    pub key: String,
    /// Its value as written in the file (G-code and lists included verbatim).
    pub raw: String,
    /// How it fared.
    pub outcome: ImportOutcome,
    /// What it set here. Empty for anything dropped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<ImportedValue>,
    /// One or two sentences on what changed and what it means for a print.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The profile an import produced, tagged with its kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "profile", rename_all = "snake_case")]
pub enum ImportedBody {
    /// A printer (machine) profile.
    Printer(PrinterProfile),
    /// A filament (material) profile.
    Filament(FilamentProfile),
    /// A print (process) profile.
    Process(ProcessProfile),
}

/// One profile made from the file, with the account of how.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImportedProfile {
    /// The profile and its kind.
    #[serde(flatten)]
    pub body: ImportedBody,
    /// What became of every setting read for it, in file order.
    pub entries: Vec<ImportEntry>,
    /// Things worth knowing about the profile as a whole — a parent preset that
    /// wasn't in the file, a value assumed because the file had none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<ImportNote>,
}

/// How much a note matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoteLevel {
    /// Worth knowing.
    Info,
    /// Check this before printing.
    Warning,
}

/// A remark about a file or a profile rather than one setting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImportNote {
    /// How much it matters.
    pub level: NoteLevel,
    /// What to know, in a sentence or two.
    pub text: String,
}

impl ImportNote {
    pub(crate) fn info(text: impl Into<String>) -> Self {
        Self {
            level: NoteLevel::Info,
            text: text.into(),
        }
    }

    pub(crate) fn warning(text: impl Into<String>) -> Self {
        Self {
            level: NoteLevel::Warning,
            text: text.into(),
        }
    }
}

/// The slicer that wrote the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceSlicer {
    /// PrusaSlicer.
    PrusaSlicer,
    /// SuperSlicer.
    SuperSlicer,
    /// The original Slic3r.
    Slic3r,
    /// OrcaSlicer.
    OrcaSlicer,
    /// Bambu Studio.
    BambuStudio,
    /// This slicer's own G-code.
    ColdCrabby,
    /// Not stated in the file.
    Unknown,
}

impl SourceSlicer {
    /// The name a person would use.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::PrusaSlicer => "PrusaSlicer",
            Self::SuperSlicer => "SuperSlicer",
            Self::Slic3r => "Slic3r",
            Self::OrcaSlicer => "OrcaSlicer",
            Self::BambuStudio => "Bambu Studio",
            Self::ColdCrabby => "Cold Crabby",
            Self::Unknown => "another slicer",
        }
    }
}

/// The kind of file the settings came out of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceContainer {
    /// A single exported configuration (`key = value` lines).
    Ini,
    /// A configuration bundle with `[print:…]` / `[filament:…]` /
    /// `[printer:…]` sections.
    IniBundle,
    /// One preset as JSON.
    Json,
    /// A zipped bundle of JSON presets.
    JsonBundle,
    /// A project file (`.3mf` / `.amf`) carrying the settings it was saved with.
    Project,
    /// The configuration block at the end of a sliced G-code file.
    Gcode,
}

impl SourceContainer {
    /// The name a person would use.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Ini => "configuration",
            Self::IniBundle => "configuration bundle",
            Self::Json => "preset",
            Self::JsonBundle => "preset bundle",
            Self::Project => "project",
            Self::Gcode => "G-code file",
        }
    }
}

/// Where the settings came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImportSource {
    /// The file name as given.
    pub file_name: String,
    /// The slicer that wrote it.
    pub slicer: SourceSlicer,
    /// That slicer's version, when the file states it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The kind of file.
    pub container: SourceContainer,
}

/// Everything one import produced: profiles ready to add, and the report.
///
/// Nothing is saved by the importer itself — the caller shows the report and
/// adds the profiles the user keeps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImportResult {
    /// Where the settings came from.
    pub source: ImportSource,
    /// The profiles made from it, printers first, then filaments, then print
    /// profiles.
    pub profiles: Vec<ImportedProfile>,
    /// Remarks about the file as a whole.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<ImportNote>,
}
