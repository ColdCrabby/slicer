//! Arachne variable-width perimeter generator.
//!
//! A medial-axis-based [Arachne][arachne-paper]-style generator (Kuipers et al.
//! 2020): concentric full-width perimeter loops whose *count* varies locally
//! (offsets vanish in thin regions), plus a variable-width bead that follows
//! the polygon's medial axis to fill thin/tapering features the loops cannot.
//! It is the generator CuraEngine / PrusaSlicer / OrcaSlicer ship as their
//! "Arachne" wall mode.
//!
//! ## Pipeline
//!
//! * [`voronoi`] — segment Voronoi diagram (BSL-1.0 `boostvoronoi`).
//! * [`skeleton`] — interior medial-axis graph with a per-node width field.
//! * [`beading`] — bead-count / width strategy (foundation for the full
//!   skeletal-trapezoidation walk; see its module note).
//! * [`generate`] — assembles the above into wall beads (the public entry).
//!
//! ## Scope
//!
//! The outer loops are constant width; continuous per-vertex width variation of
//! *every* bead (full skeletal trapezoidation) is the documented next step.
//! Selecting [`WallGenerator::Arachne`](crate::settings::params::WallGenerator)
//! runs this generator; the pure-Rust dependency stack compiles to wasm, so no
//! fallback to Classic is required (a Voronoi build error degrades gracefully
//! to the offset loops alone).
//!
//! [arachne-paper]: https://dl.acm.org/doi/10.1145/3386569.3392408

mod beading;
mod generate;
mod skeleton;
mod voronoi;

pub use generate::generate_arachne_walls;
