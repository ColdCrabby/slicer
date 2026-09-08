//! Dialect implementations for the supported firmware flavors.
//!
//! Each dialect is a concrete implementation of [`crate::gcode::GcodeDialect`].
//! Add new firmware flavors here as separate submodules.

pub mod klipper;
pub mod marlin;
pub mod reprap;

pub use klipper::KlipperDialect;
pub use marlin::MarlinDialect;
pub use reprap::RepRapDialect;
