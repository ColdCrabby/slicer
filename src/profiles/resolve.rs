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
//!   → printer.params              (hardware)
//!   → filament.params             (material)
//!   → process.params              (quality)   ← wins on shared keys (e.g. print_speed)
//!   → printer.material_overlays[] (this machine, this material family)
//!   → overrides                   ← the user's explicit deviations win over everything
//! ```
//!
//! The machine's material correction lands **after** the recipe and **before**
//! the user: a quality preset cannot undo what the hardware does with a
//! material, and the user can undo both.

use serde::{Deserialize, Serialize};

use crate::settings::params::SlicingParams;

use super::filament::FilamentProfile;
use super::library::ProfileLibrary;
use super::printer::PrinterProfile;
use super::process::ProcessProfile;

/// How a slice request names one of its three profiles.
///
/// The normal form is [`ProfileRef::Id`] — the client names a profile the
/// engine already holds, because the library lives with the engine and the UI
/// writes through to it on every edit. Sending the whole profile instead would
/// mean the client's copy silently wins over the engine's, which is the wrong
/// way round: an edit made in another tab, or by another person on a
/// self-hosted instance, would be undone by whichever browser sliced last.
///
/// [`ProfileRef::Inline`] is the fallback for a host with no library to look
/// in — the in-browser wasm build, where the browser *is* the engine and there
/// is no `profiles.toml` behind it.
///
/// Untagged on the wire, so a reference is just its id string and an inline
/// profile is just the object:
///
/// ```json
/// { "printer": "builtin-generic-printer", "filament": "builtin-generic-pla" }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum ProfileRef<T> {
    /// Resolve by id against the engine's own library.
    Id(String),
    /// The profile itself, for a host that has no library.
    Inline(Box<T>),
}

impl<T> ProfileRef<T> {
    /// The id this reference names, or `None` when it carries the profile.
    pub fn id(&self) -> Option<&str> {
        match self {
            ProfileRef::Id(id) => Some(id.as_str()),
            ProfileRef::Inline(_) => None,
        }
    }
}

/// Something a slice request named that the engine cannot resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownProfile {
    /// Which category the id was looked up in (`"printer"`, …).
    pub kind: &'static str,
    /// The id the request named.
    pub id: String,
}

impl std::fmt::Display for UnknownProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "this slicer has no {} profile with id '{}' — it may have been deleted, \
             or created in a browser that has not synced yet",
            self.kind, self.id
        )
    }
}

impl std::error::Error for UnknownProfile {}

/// Why a selection could not be turned into [`SlicingParams`].
#[derive(Debug)]
pub enum ResolveError {
    /// A named profile is not in the library (and was not sent inline).
    Unknown(UnknownProfile),
    /// The composed document is not a valid [`SlicingParams`].
    Invalid(serde_json::Error),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::Unknown(e) => e.fmt(f),
            ResolveError::Invalid(e) => write!(f, "invalid slicing parameters: {e}"),
        }
    }
}

impl std::error::Error for ResolveError {}

impl From<serde_json::Error> for ResolveError {
    fn from(e: serde_json::Error) -> Self {
        ResolveError::Invalid(e)
    }
}

/// A complete profile selection plus the user's sparse override diff.
///
/// `overrides` is a partial [`SlicingParams`] object — only the keys the user
/// changed away from the resolved profile stack need be present.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProfileSelection {
    /// Active printer (hardware) profile, by id or inline.
    pub printer: ProfileRef<PrinterProfile>,
    /// Active filament (material) profile, by id or inline.
    pub filament: ProfileRef<FilamentProfile>,
    /// Active process (quality) profile, by id or inline.
    pub process: ProfileRef<ProcessProfile>,
    /// Sparse [`SlicingParams`] overlay of user deviations. `null`/absent = none.
    #[serde(default)]
    pub overrides: serde_json::Value,
}

/// Pick one slot's profile out of `library`, or take the one sent inline.
fn pick<'a, T>(
    slot: &'a ProfileRef<T>,
    kind: &'static str,
    lookup: impl FnOnce(&'a ProfileLibrary, &str) -> Option<&'a T>,
    library: Option<&'a ProfileLibrary>,
) -> Result<&'a T, ResolveError> {
    match slot {
        ProfileRef::Inline(profile) => Ok(profile),
        ProfileRef::Id(id) => library.and_then(|lib| lookup(lib, id)).ok_or_else(|| {
            ResolveError::Unknown(UnknownProfile {
                kind,
                id: id.clone(),
            })
        }),
    }
}

impl ProfileSelection {
    /// Resolve this selection into a concrete [`SlicingParams`].
    ///
    /// `library` is the engine's own profile library, and is what every
    /// [`ProfileRef::Id`] is looked up in. Pass `None` only where there is no
    /// library — the wasm build — in which case a selection must carry its
    /// profiles inline.
    pub fn resolve(&self, library: Option<&ProfileLibrary>) -> Result<SlicingParams, ResolveError> {
        let printer = pick(&self.printer, "printer", ProfileLibrary::printer, library)?;
        let filament = pick(
            &self.filament,
            "filament",
            ProfileLibrary::filament,
            library,
        )?;
        let process = pick(&self.process, "process", ProfileLibrary::process, library)?;
        Ok(resolve(printer, filament, process, &self.overrides)?)
    }
}

/// Which layer of the profile stack supplied a resolved value.
///
/// The stack is five deep, which is only comprehensible if the user can see
/// which layer won a given setting. This is what a client shows next to a field
/// so a number that disagrees with the filament profile explains itself on the
/// spot instead of looking like a bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ParamOrigin {
    /// The engine's own default — no profile named this setting.
    Default,
    /// The printer profile's `params`.
    Printer,
    /// The filament profile's `params` (or its typed domain fields).
    Filament,
    /// The process profile's `params`.
    Process,
    /// This machine's correction for the chosen material family.
    MachineMaterial,
    /// The user's own override diff.
    Override,
}

/// Where every resolved setting came from, keyed by `SlicingParams` field name.
pub type ParamOrigins = std::collections::BTreeMap<String, ParamOrigin>;

/// Compose the profile `params` bundles + overrides into a flat
/// [`SlicingParams`].
pub fn resolve(
    printer: &PrinterProfile,
    filament: &FilamentProfile,
    process: &ProcessProfile,
    overrides: &serde_json::Value,
) -> Result<SlicingParams, serde_json::Error> {
    resolve_with_origins(printer, filament, process, overrides).map(|(params, _)| params)
}

/// [`resolve`], also reporting which layer supplied each setting.
///
/// The origins are a by-product of the same single merge — there is no second
/// pass and no second precedence table to keep in step.
pub fn resolve_with_origins(
    printer: &PrinterProfile,
    filament: &FilamentProfile,
    process: &ProcessProfile,
    overrides: &serde_json::Value,
) -> Result<(SlicingParams, ParamOrigins), serde_json::Error> {
    let mut base = serde_json::to_value(SlicingParams::default())?;
    let mut origins = ParamOrigins::new();

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

    // What this machine does differently with this *family* of material. It
    // lands after the recipe and before the user: a process profile must not be
    // able to undo a hardware fact, and the user must always be able to.
    let machine_material = printer
        .material_overlays
        .get(&filament.material)
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    for (overlay, origin) in [
        (&printer.params, ParamOrigin::Printer),
        (&filament_overlay, ParamOrigin::Filament),
        (&process.params, ParamOrigin::Process),
        (&machine_material, ParamOrigin::MachineMaterial),
        (overrides, ParamOrigin::Override),
    ] {
        let Some(map) = overlay.as_object() else {
            continue;
        };
        // Origins are tracked per top-level key — the granularity a client
        // renders a field at. A nested merge still attributes the whole field
        // to the layer that last named it, which is what "who decided this"
        // means to someone looking at one input box.
        for key in map.keys() {
            origins.insert(key.clone(), origin);
        }
        deep_merge(&mut base, overlay);
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
    for (key, origin) in [
        ("filament_type", ParamOrigin::Filament),
        ("filament_name", ParamOrigin::Filament),
        ("filament_color", ParamOrigin::Filament),
        ("printer_vendor", ParamOrigin::Printer),
        ("printer_model", ParamOrigin::Printer),
    ] {
        origins.insert(key.to_string(), origin);
    }

    // Every setting no layer named is the engine's own default, and saying so
    // is the point: a client that only knew the overridden keys would leave the
    // rest unexplained.
    if let Some(map) = base.as_object() {
        for key in map.keys() {
            origins.entry(key.clone()).or_insert(ParamOrigin::Default);
        }
    }

    serde_json::from_value(base).map(|params| (params, origins))
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
