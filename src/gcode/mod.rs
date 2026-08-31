//! G-code generation from sliced layers.
//!
//! Converts a `Vec<SliceLayer>` and a `SlicingParams` into a G-code string
//! suitable for FFF (fused-filament fabrication) printers.
//!
//! ## Architecture
//!
//! ```text
//! GcodeGenerator  ──uses──►  GcodeDialect (trait)
//!                                 ▲           ▲
//!                          MarlinDialect  KlipperDialect
//! ```
//!
//! | Module        | Contents                                              |
//! |---------------|-------------------------------------------------------|
//! | `flavor`      | [`GcodeFlavor`] enum + `FromStr` / `Display`          |
//! | `dialect`     | [`GcodeDialect`] trait (incl. `header`) + [`WarnFn`]  |
//! | `generator`   | [`GcodeGenerator`] façade + [`generate_gcode`]        |
//! | `stats`       | [`SliceStatistics`] + metadata header rendering       |
//! | `source`      | [`resolve_gcode_source`] file/string resolver         |
//! | `dialects/`   | Concrete dialect implementations (Marlin, Klipper)   |
//!
//! ## Example
//!
//! ```rust
//! use slicer_engine::gcode::{GcodeGenerator, GcodeFlavor};
//! use slicer_engine::settings::params::SlicingParams;
//!
//! let gen = GcodeGenerator::new(GcodeFlavor::Klipper);
//! let gcode = gen.generate(&[], &SlicingParams::default());
//! assert!(gcode.contains("START_PRINT"));
//! ```

pub mod dialect;
pub mod dialects;
pub mod flavor;
pub mod generator;
pub mod simplify;
pub mod source;
pub mod stats;
pub mod time_estimate;
pub mod travel;

pub use dialect::{GcodeDialect, WarnFn};
pub use dialects::{KlipperDialect, MarlinDialect};
pub use flavor::GcodeFlavor;
pub use generator::{
    generate_gcode, generate_gcode_for_plate, generate_gcode_from_params, GcodeGenerator,
};
pub use source::resolve_gcode_source;
pub use stats::SliceStatistics;
