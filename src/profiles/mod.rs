//! Profile system — the engine-owned single source of truth for printer,
//! filament, and process (print/quality) profiles.
//!
//! # Why this module exists
//!
//! Historically the Angular UI owned the profile *knowledge*: it held the
//! definitions, the built-in catalog, and the logic that flattened a
//! printer + filament + process into a [`crate::settings::params::SlicingParams`]
//! blob, which it then shipped to the slicer.  That put the domain model on the
//! wrong side of the wire and duplicated it per client.
//!
//! This module inverts that.  The **engine** owns:
//!
//! - the profile **definitions** ([`PrinterProfile`], [`FilamentProfile`],
//!   [`ProcessProfile`]) and their JSON schema (from which the UI's TypeScript
//!   types are generated),
//! - the built-in **defaults** and the bundled **catalog** ([`catalog`]),
//! - the **resolution** rules ([`resolve`]) that compose profiles + a sparse
//!   user override diff into a concrete `SlicingParams`.
//!
//! The frontend keeps only user-owned profile *instances* (persisted
//! client-side) and the user's override diff.  On slice it sends a
//! [`ProfileSelection`]; the engine resolves it.  Native, server, and the
//! in-browser WASM build all share this one implementation.
//!
//! # Resolution order
//!
//! ```text
//! default → printer → filament → process → printer×material → user overrides
//! ```
//!
//! Later stages win on shared keys (e.g. `process` speeds beat the printer's
//! `max_print_speed`), and the user's explicit overrides win over everything.
//!
//! The fourth stage is the machine's own correction for the chosen *material
//! family* — [`PrinterProfile::material_overlays`]. It sits where it does
//! because of one rule: a quality recipe must not be able to undo a hardware
//! fact, and the user must always be able to. See [`printer`] for why some
//! settings belong to a machine and a material together rather than to either
//! alone.

pub mod defaults;
pub mod export;
pub mod filament;
pub mod gcode_templates;
pub mod library;
pub mod meta;
pub mod printer;
pub mod process;
pub mod resolve;
pub mod toml_bridge;

// On-disk profile persistence (TOML). Native/server only — the wasm build has
// no filesystem and keeps the library in the browser's localStorage instead.
// The library *shape* and its TOML rendering live in `library` / `toml_bridge`,
// which every target (including wasm, for export) compiles.
#[cfg(not(target_arch = "wasm32"))]
pub mod store;

#[cfg(all(target_arch = "wasm32", feature = "web-slicer"))]
pub mod wasm;

pub use export::{export_library, ProfileExportArtifact, ProfileExportFormat};
pub use filament::{material_density, FilamentMaterial, FilamentProfile};
pub use gcode_templates::{GcodeTemplate, TemplateFlavor, GCODE_TEMPLATES};
pub use library::{Label, LabelTone, ProfileKind, ProfileLibrary};
pub use meta::{ProfileMeta, ProfileSource};
pub use printer::{BedShape, PrinterConnection, PrinterConnectionKind, PrinterProfile};
pub use process::{PrintQuality, ProcessProfile};
pub use resolve::{
    resolve, resolve_with_origins, ParamOrigin, ParamOrigins, ProfileRef, ProfileSelection,
    ResolveError, UnknownProfile,
};

#[cfg(not(target_arch = "wasm32"))]
pub use store::ProfileStore;

#[cfg(test)]
mod tests {
    use super::*;

    /// A selection that carries its profiles inline, the way the wasm build
    /// (which has no library to look an id up in) sends them.
    fn selection() -> ProfileSelection {
        ProfileSelection {
            printer: resolve::ProfileRef::Inline(Box::new(defaults::default_printer())),
            filament: resolve::ProfileRef::Inline(Box::new(defaults::default_filament())),
            process: resolve::ProfileRef::Inline(Box::new(defaults::default_process())),
            overrides: serde_json::Value::Null,
        }
    }

    #[test]
    fn resolve_composes_all_three_profiles() {
        let params = selection().resolve(None).expect("resolve");
        // Printer-owned.
        assert_eq!(params.nozzle_diameter_mm, 0.4);
        assert_eq!(params.retract_mm, 0.8);
        assert_eq!(params.retract_speed_mm_min, 40.0 * 60.0);
        // Filament-owned (fan speeds are engine-native fractions).
        assert_eq!(params.nozzle_temp, 210.0);
        assert_eq!(params.fan_speed, 1.0);
        // Process-owned wins on shared `print_speed`.
        assert_eq!(params.print_speed, 120.0);
        assert_eq!(params.layer_height, 0.2);
        assert_eq!(params.first_layer_height, 0.24);
    }

    #[test]
    fn resolve_stamps_filament_identity_and_density() {
        let params = selection().resolve(None).expect("resolve");
        // Identity / display fields come from the chosen filament profile and
        // are surfaced in the G-code metadata footer.
        assert_eq!(params.filament_type, "PLA");
        assert_eq!(params.filament_name, "Generic PLA");
        assert_eq!(params.filament_color, "#d8d8dc");
        // Density is folded in from the profile's typed domain field.
        assert_eq!(params.filament_density_g_cm3, 1.24);
    }

    /// Machine identity and filament price reach the G-code metadata footer, so
    /// Moonraker / OctoPrint can show which printer a job was sliced for and
    /// what the material cost (issue #23).
    #[test]
    fn resolve_stamps_machine_identity_and_filament_price() {
        let params = selection().resolve(None).expect("resolve");
        assert_eq!(params.printer_vendor, "Generic");
        assert_eq!(params.printer_model, "FDM 220");
        assert_eq!(params.filament_cost_per_kg, 25.0);
    }

    #[test]
    fn override_price_wins_over_profile_price() {
        // Price is folded in at the *filament* precedence layer, like density,
        // so an explicit user override still wins.
        let mut sel = selection();
        sel.overrides = serde_json::json!({ "filament_cost_per_kg": 42.0 });
        let params = sel.resolve(None).expect("resolve");
        assert_eq!(params.filament_cost_per_kg, 42.0);
    }

    #[test]
    fn resolve_uses_material_density_for_weight() {
        // A PETG filament (1.27 g/cm³) must not fall back to the PLA default
        // (1.24) — otherwise the metadata weight would be wrong for non-PLA.
        let mut sel = selection();
        sel.filament = resolve::ProfileRef::Inline(Box::new(defaults::default_petg()));
        let params = sel.resolve(None).expect("resolve");
        assert_eq!(params.filament_type, "PETG");
        assert_eq!(params.filament_name, "Generic PETG");
        // Read the swatch off the preset rather than pinning the hex: the
        // colour is decoration, and what this line is really checking is that
        // the inline profile's own fields reach the resolved params.
        assert_eq!(params.filament_color, defaults::default_petg().color);
        assert_eq!(params.filament_density_g_cm3, 1.27);
    }

    #[test]
    fn override_density_wins_over_profile_density() {
        // The folded profile density resolves at the *filament* layer, so an
        // explicit user override still wins.
        let mut sel = selection();
        sel.overrides = serde_json::json!({ "filament_density_g_cm3": 2.0 });
        let params = sel.resolve(None).expect("resolve");
        assert_eq!(params.filament_density_g_cm3, 2.0);
    }

    #[test]
    fn overrides_win_over_profiles() {
        let mut sel = selection();
        sel.overrides = serde_json::json!({
            "layer_height": 0.15,
            "nozzle_temp": 225.0,
            "adhesion_type": "brim",
        });
        let params = sel.resolve(None).expect("resolve");
        assert_eq!(params.layer_height, 0.15);
        assert_eq!(params.nozzle_temp, 225.0);
        assert_eq!(
            params.adhesion_type,
            crate::settings::params::AdhesionType::Brim
        );
        // Untouched keys keep their resolved values.
        assert_eq!(params.print_speed, 120.0);
    }

    /// A machine that melts PLA faster than the generic spool value says so on
    /// the printer, once, and every PLA profile inherits it.
    #[test]
    fn a_machine_material_overlay_corrects_the_resolved_material() {
        let mut printer = defaults::default_printer();
        printer.material_overlays.insert(
            FilamentMaterial::PLA,
            serde_json::json!({ "max_volumetric_speed": 24.0 }),
        );

        let mut sel = selection();
        sel.printer = resolve::ProfileRef::Inline(Box::new(printer));
        let params = sel.resolve(None).expect("resolve");

        assert_eq!(params.max_volumetric_speed, 24.0);
    }

    /// The overlay is keyed on the *family*, so it must not leak onto a
    /// different one — that would make it a second global setting.
    #[test]
    fn a_machine_material_overlay_only_applies_to_its_own_material() {
        let mut printer = defaults::default_printer();
        printer.material_overlays.insert(
            FilamentMaterial::PLA,
            serde_json::json!({ "max_volumetric_speed": 24.0 }),
        );

        let mut sel = selection();
        sel.printer = resolve::ProfileRef::Inline(Box::new(printer));
        sel.filament = resolve::ProfileRef::Inline(Box::new(defaults::default_petg()));
        let params = sel.resolve(None).expect("resolve");

        // PETG's own 12 mm³/s, untouched by the PLA correction.
        assert_eq!(params.max_volumetric_speed, 12.0);
    }

    /// The precedence the whole design rests on: a quality recipe cannot undo a
    /// hardware fact, and the user can always undo both.
    #[test]
    fn a_machine_material_overlay_beats_the_process_but_loses_to_the_user() {
        let mut printer = defaults::default_printer();
        printer.material_overlays.insert(
            FilamentMaterial::PLA,
            serde_json::json!({ "nozzle_temp": 205.0 }),
        );
        let mut process = defaults::default_process();
        process
            .params
            .as_object_mut()
            .expect("params object")
            .insert("nozzle_temp".to_string(), serde_json::json!(230.0));

        let mut sel = selection();
        sel.printer = resolve::ProfileRef::Inline(Box::new(printer));
        sel.process = resolve::ProfileRef::Inline(Box::new(process));

        assert_eq!(
            sel.resolve(None).expect("resolve").nozzle_temp,
            205.0,
            "the machine's correction must win over the recipe"
        );

        sel.overrides = serde_json::json!({ "nozzle_temp": 212.0 });
        assert_eq!(
            sel.resolve(None).expect("resolve").nozzle_temp,
            212.0,
            "the user must win over the machine's correction"
        );
    }

    /// Pressure advance is tuned on the machine and read off its config. A
    /// generic value on the filament — which resolves *above* the printer —
    /// would overwrite that calibration on every slice.
    #[test]
    fn a_machines_tuned_pressure_advance_survives_the_filament() {
        let mut printer = defaults::default_printer();
        printer
            .params
            .as_object_mut()
            .expect("params object")
            .insert("pressure_advance".to_string(), serde_json::json!(0.032));

        let mut sel = selection();
        sel.printer = resolve::ProfileRef::Inline(Box::new(printer));

        assert_eq!(sel.resolve(None).expect("resolve").pressure_advance, 0.032);
    }

    /// Five layers are only comprehensible if a client can say which one won.
    #[test]
    fn resolution_reports_where_each_setting_came_from() {
        let mut printer = defaults::default_printer();
        printer.material_overlays.insert(
            FilamentMaterial::PLA,
            serde_json::json!({ "max_volumetric_speed": 24.0 }),
        );

        let (_, origins) = resolve_with_origins(
            &printer,
            &defaults::default_filament(),
            &defaults::default_process(),
            &serde_json::json!({ "layer_height": 0.15 }),
        )
        .expect("resolve");

        let origin = |key: &str| *origins.get(key).expect("every setting has an origin");
        assert_eq!(origin("nozzle_diameter_mm"), ParamOrigin::Printer);
        assert_eq!(origin("bed_temp"), ParamOrigin::Filament);
        assert_eq!(origin("infill_density"), ParamOrigin::Process);
        assert_eq!(origin("max_volumetric_speed"), ParamOrigin::MachineMaterial);
        assert_eq!(origin("layer_height"), ParamOrigin::Override);
        // Nothing names it, so it is the engine's own — and saying so is the
        // point: an unexplained setting is what the provenance exists to fix.
        assert_eq!(origin("xy_size_compensation"), ParamOrigin::Default);
    }

    #[test]
    fn empty_overrides_are_a_no_op() {
        let mut sel = selection();
        sel.overrides = serde_json::json!({});
        let a = sel.resolve(None).expect("resolve");
        let b = selection().resolve(None).expect("resolve");
        assert_eq!(a, b);
    }

    #[test]
    fn default_profiles_round_trip_through_json() {
        let printer = defaults::default_printer();
        let json = serde_json::to_string(&printer).expect("serialize");
        let back: PrinterProfile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.meta.id, "builtin-generic-printer");
        assert_eq!(back.meta.source, ProfileSource::Builtin);
    }
}
