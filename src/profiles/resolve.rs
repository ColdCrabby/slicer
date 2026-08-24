//! Profile resolution — the single place that turns a profile *selection* plus
//! the user's sparse *override diff* into a flat [`SlicingParams`].
//!
//! Because every profile carries its slice contribution as a sparse
//! `SlicingParams` object (same field names, same units), resolution is a plain
//! JSON deep-merge in a fixed precedence — there is no per-field mapping or unit
//! translation anywhere:
//!
//! ```text
//! SlicingParams::default()
//!   → printer.params      (hardware)
//!   → filament.params     (material)
//!   → process.params      (quality)   ← wins on shared keys (e.g. print_speed)
//!   → overrides           ← the user's explicit deviations win over everything
//! ```

use serde::{Deserialize, Serialize};

use crate::settings::params::SlicingParams;

use super::filament::FilamentProfile;
use super::printer::PrinterProfile;
use super::process::ProcessProfile;

/// A complete profile selection plus the user's sparse override diff.
///
/// `overrides` is a partial [`SlicingParams`] object — only the keys the user
/// changed away from the resolved profile stack need be present.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProfileSelection {
    /// Active printer (hardware) profile.
    pub printer: PrinterProfile,
    /// Active filament (material) profile.
    pub filament: FilamentProfile,
    /// Active process (quality) profile.
    pub process: ProcessProfile,
    /// Sparse [`SlicingParams`] overlay of user deviations. `null`/absent = none.
    #[serde(default)]
    pub overrides: serde_json::Value,
}

impl ProfileSelection {
    /// Resolve this selection into a concrete [`SlicingParams`].
    pub fn resolve(&self) -> Result<SlicingParams, serde_json::Error> {
        resolve(
            &self.printer,
            &self.filament,
            &self.process,
            &self.overrides,
        )
    }
}

/// Compose the profile `params` bundles + overrides into a flat
/// [`SlicingParams`].
pub fn resolve(
    printer: &PrinterProfile,
    filament: &FilamentProfile,
    process: &ProcessProfile,
    overrides: &serde_json::Value,
) -> Result<SlicingParams, serde_json::Error> {
    let mut base = serde_json::to_value(SlicingParams::default())?;
    for overlay in [
        &printer.params,
        &filament.params,
        &process.params,
        overrides,
    ] {
        if overlay.is_object() {
            deep_merge(&mut base, overlay);
        }
    }
    // The material family is a typed field on the filament profile, not part of
    // its sparse `params`; stamp it in last so `{filament_type}` in custom
    // G-code always reflects the active filament even if its params omit it.
    if let Some(map) = base.as_object_mut() {
        map.insert(
            "filament_type".to_string(),
            serde_json::Value::String(filament.material.wire_name().to_string()),
        );
    }
    serde_json::from_value(base)
}

/// Recursively merge `overlay` into `base`, mutating `base` in place.
///
/// Objects merge key-by-key; every other kind (scalars, arrays) replaces
/// wholesale. `null` values in the overlay are treated as an explicit reset to
/// that key (they overwrite the base value).
fn deep_merge(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(overlay_map)) => {
            for (key, overlay_val) in overlay_map {
                match base_map.get_mut(key) {
                    Some(base_val) => deep_merge(base_val, overlay_val),
                    None => {
                        base_map.insert(key.clone(), overlay_val.clone());
                    }
                }
            }
        }
        (base_slot, overlay_val) => {
            *base_slot = overlay_val.clone();
        }
    }
}
