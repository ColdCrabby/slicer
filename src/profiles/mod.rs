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
//! default → printer → filament → process → user overrides
//! ```
//!
//! Later stages win on shared keys (e.g. `process` speeds beat the printer's
//! `max_print_speed`), and the user's explicit overrides win over everything.

pub mod defaults;
pub mod filament;
pub mod meta;
pub mod printer;
pub mod process;
pub mod resolve;

#[cfg(all(target_arch = "wasm32", feature = "web-slicer"))]
pub mod wasm;

pub use filament::{material_density, FilamentMaterial, FilamentProfile};
pub use meta::{ProfileMeta, ProfileSource};
pub use printer::{BedShape, PrinterConnection, PrinterConnectionKind, PrinterProfile};
pub use process::{PrintQuality, ProcessProfile};
pub use resolve::{resolve, ProfileSelection};

#[cfg(test)]
mod tests {
    use super::*;

    fn selection() -> ProfileSelection {
        ProfileSelection {
            printer: defaults::default_printer(),
            filament: defaults::default_filament(),
            process: defaults::default_process(),
            overrides: serde_json::Value::Null,
        }
    }

    #[test]
    fn resolve_composes_all_three_profiles() {
        let params = selection().resolve().expect("resolve");
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
    fn overrides_win_over_profiles() {
        let mut sel = selection();
        sel.overrides = serde_json::json!({
            "layer_height": 0.15,
            "nozzle_temp": 225.0,
            "adhesion_type": "brim",
        });
        let params = sel.resolve().expect("resolve");
        assert_eq!(params.layer_height, 0.15);
        assert_eq!(params.nozzle_temp, 225.0);
        assert_eq!(
            params.adhesion_type,
            crate::settings::params::AdhesionType::Brim
        );
        // Untouched keys keep their resolved values.
        assert_eq!(params.print_speed, 120.0);
    }

    #[test]
    fn empty_overrides_are_a_no_op() {
        let mut sel = selection();
        sel.overrides = serde_json::json!({});
        let a = sel.resolve().expect("resolve");
        let b = selection().resolve().expect("resolve");
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
