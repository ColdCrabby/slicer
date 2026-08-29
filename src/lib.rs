//! # Slicer Engine
//!
//! A high-performance 3D model slicer engine written in Rust.
//! Powered by Clipper2 for robust polygon clipping operations.
//!
//! ## Features
//! - Cross-platform support (Windows, macOS, iOS/iPadOS, WebAssembly)
//! - Optimized for multi-threaded environments
//! - Type-safe geometric operations
//! - Mesh loading and spatial analysis (STL binary/ASCII)
//! - Printer profile and slicing parameter validation
//! - User-friendly CLI layer for command-line usage

#[cfg(any(not(target_arch = "wasm32"), feature = "web-slicer"))]
pub mod adhesion;
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
#[cfg(any(not(target_arch = "wasm32"), feature = "web-slicer"))]
pub mod profiles;
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

// `cli`, `db` and `server` are host-only: they are the command line, the SQLite
// history/cache store and the HTTP+WebSocket surface, none of which an iOS app
// links (it drives the engine through `tauri::invoke` instead, and a sandboxed
// mobile app must not bind a listener). Excluding them here is what lets
// `Cargo.toml` keep clap, sea-orm/sqlx and actix-web off the `aarch64-apple-ios*`
// targets entirely — keep the two in sync.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "ios")))]
pub mod cli;

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "ios")))]
pub mod db;

/// Outbound printer transports (Moonraker/Klipper, …). Native only — a browser
/// wasm build talks to printers directly over `fetch` instead (CORS-permitting).
/// Kept on iOS: sending G-code to a printer from an iPad goes through this same
/// native path, which is what keeps it clear of the browser's CORS restrictions.
#[cfg(not(target_arch = "wasm32"))]
pub mod printer;

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "ios")))]
pub mod server;

#[cfg(any(not(target_arch = "wasm32"), feature = "web-slicer"))]
pub use core::*;
