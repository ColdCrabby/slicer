//! # Slicer Engine
//!
//! A high-performance 3D model slicer engine written in Rust.
//! Powered by Clipper2 for robust polygon clipping operations.
//!
//! ## Features
//! - Cross-platform support (Windows, macOS, WebAssembly)
//! - Optimized for multi-threaded environments
//! - Type-safe geometric operations
//! - Mesh loading and spatial analysis (STL binary/ASCII)
//! - Printer profile and slicing parameter validation
//! - User-friendly CLI layer for command-line usage

#[cfg(any(not(target_arch = "wasm32"), feature = "web-slicer"))]
pub mod core;
#[cfg(any(not(target_arch = "wasm32"), feature = "web-slicer"))]
pub mod flow;
#[cfg(any(not(target_arch = "wasm32"), feature = "web-slicer"))]
pub mod gcode;
#[cfg(any(not(target_arch = "wasm32"), feature = "web-slicer"))]
pub mod infill;
pub mod logging;
pub mod mesh;
pub mod orient;
pub mod scene;
#[cfg(any(not(target_arch = "wasm32"), feature = "web-slicer"))]
pub mod settings;
/// Build-time version + embedded changelog — the single source of truth
/// surfaced to every target (CLI, WS server, WASM/UI, desktop).
pub mod version;
#[cfg(any(not(target_arch = "wasm32"), feature = "web-slicer"))]
pub mod walls;

// The G-code viewer's parser/types are platform-independent and unit-tested on
// the host; only its wasm-bindgen surface (in `wasm.rs`) is gated to wasm32.
pub mod gcode_viewer;

// Provide C++ operator new/delete and __libcpp_verbose_abort so the linker
// resolves these internally instead of leaving them as WASM "env" imports
// (which wasm-bindgen would emit as unresolvable ES module imports).
#[cfg(all(target_arch = "wasm32", feature = "web-slicer"))]
mod cpp_shims;

#[cfg(not(target_arch = "wasm32"))]
pub mod debug;

#[cfg(not(target_arch = "wasm32"))]
pub mod config;
#[cfg(not(target_arch = "wasm32"))]
pub mod ws_protocol;

#[cfg(not(target_arch = "wasm32"))]
pub mod cli;

#[cfg(not(target_arch = "wasm32"))]
pub mod db;

#[cfg(not(target_arch = "wasm32"))]
pub mod server;

#[cfg(any(not(target_arch = "wasm32"), feature = "web-slicer"))]
pub use core::*;
