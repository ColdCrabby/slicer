//! The workplate as a *saved document* — what was on the plate, and what it was
//! set up to print with.
//!
//! A plate's scene is ephemeral: it lives in the WebSocket session's scene
//! engine and is gone when the connection drops. That is fine for slicing, and
//! wrong for everything else, because the plate is the thing the user thinks
//! they are working on. Reopening one a week later has to bring back where the
//! objects sat and which printer, filament and process it was set up with — and
//! it has to do so from wherever the engine runs, for the same reason profiles
//! do:
//!
//! > **A cloud user who clears their browser must not lose their plates.**
//!
//! [`WorkplateSetup`] is that document. It is deliberately *thin*: references,
//! not copies.
//!
//! ## The contract
//!
//! - **Presets are named, never copied.** A plate records three profile ids. It
//!   does not embed the profiles, so editing a print profile reaches every plate
//!   that uses it — which is the entire point of a profile.
//! - **Settings are stored as a diff**, against those presets. A value equal to
//!   what the preset says is not stored at all; storing it would pin the plate
//!   and stop it following the profile it was never really changed away from.
//! - **Objects are file references plus a placement.** The mesh bytes stay in
//!   the work dir under their own `file_id`; the plate says where each one sits.
//!   Ten copies of a model are ten placements and one upload.
//! - **Saving a plate is not what makes a slice correct.** A slice request still
//!   carries the scene it is slicing, in full. The saved placements are what the
//!   plate is *restored* from; the request is what is *sliced*, and the two are
//!   allowed to differ while the user is mid-edit.
//!
//! ## Non-goals
//!
//! - **No mesh bytes.** Same rule as [`crate::db`]: the filesystem is the
//!   storage, this is the index.
//! - **No merge resolution.** Whole-document, last writer wins — the plate has
//!   one editor in practice, and the sync unit matches
//!   [`ProfileLibrary`](crate::profiles::ProfileLibrary)'s.
//! - **No history.** Previous versions of a plate are not kept; that is what the
//!   slice history in [`crate::db`] is for.
//!
//! ## See also
//!
//! - [`crate::db`] — where this is persisted (a column on the plate's row)
//! - [`crate::profiles`] — the library the preset ids name
//! - [`crate::scene`] — the live, in-memory placement engine

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ws_protocol::SceneObjectSliceDto;

pub mod store;

pub use store::{workplates_dir, WorkplateStore};

/// Which presets a plate is set up with.
///
/// Ids into the engine's own [`ProfileLibrary`](crate::profiles::ProfileLibrary).
/// Each is optional so a plate saved by an older build, or one whose profile was
/// since deleted, still loads — the UI falls back to the current selection
/// rather than refusing to open the plate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkplatePresets {
    /// Printer profile id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printer: Option<String>,
    /// Filament profile id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filament: Option<String>,
    /// Process profile id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<String>,
}

/// Everything remembered about one workplate.
///
/// Every field is defaulted, so a document written by a different build loads
/// with whatever it does carry rather than failing the whole plate.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WorkplateSetup {
    /// The user's name for the plate, when they renamed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The profile stack this plate's overrides are measured against.
    #[serde(default)]
    pub presets: WorkplatePresets,
    /// Sparse `SlicingParams` overlay — only the keys the user changed.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub overrides: serde_json::Value,
    /// Where each object sits, and which uploaded file it came from.
    #[serde(default)]
    pub objects: Vec<SceneObjectSliceDto>,
    /// RFC 3339 timestamp of the last save, for last-writer-wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

impl WorkplateSetup {
    /// Whether this document is worth persisting.
    ///
    /// A plate the user has neither renamed, re-configured nor placed anything
    /// on carries no information the defaults do not already supply, and writing
    /// one on every page load would fill the table with empty rows.
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.presets == WorkplatePresets::default()
            && (self.overrides.is_null()
                || self.overrides.as_object().is_some_and(|m| m.is_empty()))
            && self.objects.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_untouched_plate_is_not_worth_storing() {
        assert!(WorkplateSetup::default().is_empty());
        assert!(WorkplateSetup {
            overrides: serde_json::json!({}),
            ..Default::default()
        }
        .is_empty());
    }

    #[test]
    fn anything_the_user_chose_makes_it_worth_storing() {
        assert!(!WorkplateSetup {
            presets: WorkplatePresets {
                filament: Some("builtin-generic-petg".into()),
                ..Default::default()
            },
            ..Default::default()
        }
        .is_empty());
        assert!(!WorkplateSetup {
            overrides: serde_json::json!({ "layer_height": 0.12 }),
            ..Default::default()
        }
        .is_empty());
    }

    /// The document is references and placements — never a copy of a profile
    /// and never mesh bytes.
    #[test]
    fn a_saved_plate_names_its_profiles_rather_than_embedding_them() {
        let setup = WorkplateSetup {
            presets: WorkplatePresets {
                printer: Some("builtin-generic-printer".into()),
                filament: Some("builtin-generic-pla".into()),
                process: Some("builtin-standard-02".into()),
            },
            overrides: serde_json::json!({ "layer_height": 0.12 }),
            objects: vec![SceneObjectSliceDto {
                file_id: "00000000-0000-0000-0000-000000000001".into(),
                part_index: 0,
                transform: Default::default(),
                support_paint: None,
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&setup).expect("serialize");
        assert!(json.contains("builtin-generic-pla"));
        assert!(!json.contains("start_gcode"), "no profile params inlined");
        assert!(json.len() < 400, "a plate is small: {} bytes", json.len());

        let back: WorkplateSetup = serde_json::from_str(&json).expect("round trip");
        assert_eq!(back, setup);
    }
}
