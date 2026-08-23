//! Process (print / quality) profile.
//!
//! A process profile is just a **named bundle of [`SlicingParams`] overrides**
//! plus a coarse quality tag — nothing is renamed or re-unitised. The `params`
//! field is a sparse `SlicingParams` object (same field names, same units as
//! the engine), so there is exactly one representation of a slice parameter and
//! no mapping layer anywhere.
//!
//! [`SlicingParams`]: crate::settings::params::SlicingParams

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::meta::ProfileMeta;

/// Coarse quality tag used for badges / sorting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrintQuality {
    /// Fast, coarse.
    Draft,
    /// Balanced default.
    #[default]
    Standard,
    /// Slow, fine detail.
    Fine,
}

/// A process (print / quality) profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProcessProfile {
    /// Provenance (id, name, source, …).
    #[serde(flatten)]
    pub meta: ProfileMeta,

    /// Coarse quality tag.
    #[serde(default)]
    pub quality: PrintQuality,

    /// Sparse `SlicingParams` overrides this profile contributes (e.g.
    /// `layer_height`, `wall_count`, `wall_generator`, `print_speed`,
    /// `infill_density`, `seam_position`, adhesion/support keys).
    #[serde(default)]
    pub params: serde_json::Value,
}
