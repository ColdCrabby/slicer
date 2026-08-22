//! WebAssembly bindings for GCode visualization.
//!
//! Parses a `.gcode` file (as raw bytes) entirely in Rust and returns
//! per-layer geometry buffers that the Angular UI hands directly to Three.js
//! `LineSegments`. No GCode parsing takes place in JavaScript.
//!
//! ## Data flow
//! ```text
//! bytes (Uint8Array)
//!   → GcodeHandle::parse()
//!       → Vec<InternalLayer>          (parser.rs)
//!           → GcodeHandle::get_layer(i)
//!               → GcodeLayerBuffer    (wasm.rs)
//!                   → Three.js LineSegments
//! ```
//!
//! Each `Float32Array` holds flat line-segment records:
//! `[x0, y0, z0,  x1, y1, z1,  width, height, speed,  …]`  (9 floats per
//! segment, where `speed` is the extrusion feedrate in mm/s).

mod parser;
mod types;
mod wasm;

pub use wasm::{GcodeHandle, GcodeLayerBuffer};
