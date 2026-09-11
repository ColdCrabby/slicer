//! Mesh loading, spatial analysis, and coordinate transformation.
//!
//! # Modules
//! - [`types`]: Core data structures (`Vertex`, `Face`, `AABB`, `Mesh`)
//! - [`io`]: STL file reading (binary and ASCII)
//! - [`analysis`]: Geometry calculations (AABB, volume, surface area)
//! - [`paint`]: Per-facet support paint (enforcers and blockers)
//! - [`repair`]: Topological validation and auto-repair on import
//! - [`transforms`]: Coordinate transforms (center, drop to floor, translate)

pub mod analysis;
pub mod io;
pub mod paint;
pub mod repair;
pub mod transforms;
pub mod types;

pub use paint::{FacetPaint, PaintDecodeError, PaintState};
pub use repair::{analyze, repair, MeshDiagnostics, MeshReport, RepairActions, RepairOptions};
pub use types::{Face, Mesh, Vertex, AABB};
