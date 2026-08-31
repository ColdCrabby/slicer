//! The user-owned profile library — the data model, independent of storage.
//!
//! [`ProfileStore`](super::store::ProfileStore) persists this to
//! `profiles.toml` on native/server targets, and
//! [`export`](super::export) renders it for download. Both need the shape, but
//! only the former needs a filesystem, so the types live here where the wasm
//! build can use them too.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{FilamentProfile, PrinterProfile, ProcessProfile};

/// Shade of a [`Label`] tint (mirrors `ui/src/app/models/label.model.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LabelTone {
    /// Deeper, slightly stronger tint.
    #[default]
    Dark,
    /// The same hue rendered more transparent, so it reads softer.
    Light,
}

/// A user-defined, cross-area label — think Finder tags with GitHub-style
/// colours. Attached to profiles via [`ProfileMeta::label_ids`], kept as a
/// single flat vocabulary rather than three per-area lists.
///
/// [`ProfileMeta::label_ids`]: super::ProfileMeta::label_ids
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Label {
    /// Stable identifier.
    pub id: String,
    /// Human-readable name shown in the UI.
    pub name: String,
    /// Base hue as a hex string (e.g. `#b8942f`).
    pub color: String,
    /// Shade applied to the base hue.
    #[serde(default)]
    pub tone: LabelTone,
}

/// The complete set of user-owned profile instances the engine persists.
///
/// Each field is a syncable category; the UI mirrors these as its four profile
/// stores. Whole-category replacement (last-writer-wins) is the sync unit — see
/// [`ProfileStore::replace_category`](super::store::ProfileStore::replace_category).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProfileLibrary {
    /// User-owned printer (machine) profiles.
    #[serde(default)]
    pub printers: Vec<PrinterProfile>,
    /// User-owned filament (material) profiles.
    #[serde(default)]
    pub filaments: Vec<FilamentProfile>,
    /// User-owned process (print/quality) profiles.
    #[serde(default)]
    pub processes: Vec<ProcessProfile>,
    /// Flat, cross-area label vocabulary.
    #[serde(default)]
    pub labels: Vec<Label>,
}

/// One syncable profile category — the granularity of a write-through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProfileKind {
    /// [`ProfileLibrary::printers`].
    Printers,
    /// [`ProfileLibrary::filaments`].
    Filaments,
    /// [`ProfileLibrary::processes`].
    Processes,
    /// [`ProfileLibrary::labels`].
    Labels,
}

impl ProfileKind {
    /// The lowercase wire/route token for this category (`"printers"`, …).
    pub fn as_str(self) -> &'static str {
        match self {
            ProfileKind::Printers => "printers",
            ProfileKind::Filaments => "filaments",
            ProfileKind::Processes => "processes",
            ProfileKind::Labels => "labels",
        }
    }

    /// Parse a category from its wire/route token (case-insensitive).
    pub fn parse(token: &str) -> Option<Self> {
        match token.to_ascii_lowercase().as_str() {
            "printers" => Some(ProfileKind::Printers),
            "filaments" => Some(ProfileKind::Filaments),
            "processes" => Some(ProfileKind::Processes),
            "labels" => Some(ProfileKind::Labels),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_kind_parses_and_stringifies() {
        for kind in [
            ProfileKind::Printers,
            ProfileKind::Filaments,
            ProfileKind::Processes,
            ProfileKind::Labels,
        ] {
            assert_eq!(ProfileKind::parse(kind.as_str()), Some(kind));
            assert_eq!(
                ProfileKind::parse(&kind.as_str().to_uppercase()),
                Some(kind)
            );
        }
        assert_eq!(ProfileKind::parse("bogus"), None);
    }
}
