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

    // Fold the filament profile's typed *domain* density into its sparse
    // `params` overlay so it resolves at the **filament** precedence layer —
    // process and the user's overrides can still win. Without this the weight in
    // the metadata footer would always use the default (PLA) density regardless
    // of the chosen material. The price per kilogram rides along the same way so
    // the footer can report a material cost. `entry` only inserts when the
    // profile's params (or a downstream layer) didn't already carry it.
    let mut filament_overlay = filament.params.clone();
    if !filament_overlay.is_object() {
        filament_overlay = serde_json::json!({});
    }
    if let Some(fmap) = filament_overlay.as_object_mut() {
        fmap.entry("filament_density_g_cm3")
            .or_insert_with(|| serde_json::Value::from(filament.density_g_cm3));
        fmap.entry("filament_cost_per_kg")
            .or_insert_with(|| serde_json::Value::from(filament.cost_per_kg));
    }

    for overlay in [
        &printer.params,
        &filament_overlay,
        &process.params,
        overrides,
    ] {
        if overlay.is_object() {
            deep_merge(&mut base, overlay);
        }
    }
    // Identity / display fields tied to the *chosen* profiles. These are
    // definitional (they name the active filament and machine), so they always
    // reflect the selected profiles regardless of the generic override diff, and
    // are surfaced in the G-code metadata footer so printer front-ends
    // (Moonraker/Mainsail/Fluidd, OctoPrint) can show the material, filament
    // name, a colour swatch, and which machine the file was sliced for.
    // `{filament_type}` is also exposed to custom start G-code.
    if let Some(map) = base.as_object_mut() {
        map.insert(
            "filament_type".to_string(),
            serde_json::Value::String(filament.material.wire_name().to_string()),
        );
        map.insert(
            "filament_name".to_string(),
            serde_json::Value::String(filament.meta.name.clone()),
        );
        map.insert(
            "filament_color".to_string(),
            serde_json::Value::String(filament.color.clone()),
        );
        map.insert(
            "printer_vendor".to_string(),
            serde_json::Value::String(printer.vendor.clone()),
        );
        map.insert(
            "printer_model".to_string(),
            serde_json::Value::String(printer.model.clone()),
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
