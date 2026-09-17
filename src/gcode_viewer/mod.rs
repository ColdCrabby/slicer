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
//! `[x0, y0, z0,  x1, y1, z1,  width, height, speed, accel,  …]`  (10 floats per
//! segment, where `speed` is the extrusion feedrate in mm/s and `accel` is the
//! commanded print acceleration in mm/s², `0` when the G-code sets none).
//!
//! Beside each block's floats sits a `Uint32Array` of the same length in
//! *segments*: the 1-based file line each move came from
//! (`GcodeLayerBuffer::block_lines`). It is what lets the UI show the preview
//! and the file side by side and keep them on the same move in both
//! directions — and, like everything else here, it is derived once during the
//! parse rather than recovered in JavaScript.

// Parsing core compiles everywhere so it can be unit-tested on the host; on
// native some of its wasm-only accessors are unused, hence the dead_code allow.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod parser;
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod types;
#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(target_arch = "wasm32")]
pub use wasm::{GcodeHandle, GcodeLayerBuffer};
