//! Provenance metadata shared by every profile kind.
//!
//! Mirrors `ui/src/app/models/profile-source.ts` so the frontend and the
//! engine speak the same shape.  The engine is the single source of truth for
//! these definitions; the UI's TypeScript types are generated from the JSON
//! schema of the profile structs.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where a profile came from. Drives badges and edit/delete affordances in the
/// settings UI.
///
/// - `builtin` — the single offline default shipped with the app for a given
///   category. Always present, editable but not deletable.
/// - `user` — created from scratch, duplicated, or imported+customised.
/// - `catalog` — a read-only entry served from the bundled catalog. Importing
///   one produces a `user` copy whose [`ProfileMeta::based_on`] points back at
///   the catalog id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProfileSource {
    /// The single offline default for a category.
    Builtin,
    /// A fully user-owned profile.
    #[default]
    User,
    /// A read-only bundled-catalog entry.
    Catalog,
}

/// Provenance fields shared by every profile kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProfileMeta {
    /// Stable identifier (uuid-ish string; catalog ids are human-readable).
    pub id: String,
    /// Human-readable name shown in the UI.
    pub name: String,
    /// Where this profile came from.
    #[serde(default)]
    pub source: ProfileSource,
    /// Catalog id this profile was imported/derived from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on: Option<String>,
    /// Provenance for a profile fetched from the frontend's cloud catalog: the
    /// source API URL it was imported from. Purely informational — the slicer
    /// never reads it; an imported profile is treated exactly like a
    /// hand-made one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_url: Option<String>,
    /// Ids of user-defined labels attached to this profile (flat, cross-area).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub label_ids: Vec<String>,
}

impl ProfileMeta {
    /// Construct a `builtin` meta with the given id + name.
    pub fn builtin(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            source: ProfileSource::Builtin,
            based_on: None,
            import_url: None,
            label_ids: Vec::new(),
        }
    }
}
