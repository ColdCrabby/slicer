//! The slicer → printer link: identifying a machine, and talking to it.
//!
//! Split in two because the halves have different reach:
//!
//! - [`klipper`] and [`detection`] are **pure** — they turn Moonraker's JSON
//!   into a [`PrinterDetection`] with no I/O, and so compile everywhere,
//!   including wasm. That is what lets the browser build reach the same
//!   conclusions about a printer as the server and the desktop app without a
//!   second implementation of the rules in TypeScript.
//! - [`transport`] is **native only**. It owns the HTTP: probing, status and
//!   G-code upload.
//!
//! The engine prefers to talk to printers **from the native process** (CLI or
//! the `serve` WebSocket server) so that the request never leaves the same
//! trust boundary as the printer's LAN and, crucially, is **not subject to
//! browser CORS**. Moonraker ships no permissive `Access-Control-*` headers by
//! default, so a direct browser `fetch` from the Angular UI fails for most
//! users. Routing the probe/upload through the server sidesteps that entirely.
//!
//! In the wasm build there is no native transport; there the UI falls back to a
//! direct `fetch`, which is expected to fail on CORS for many hosts and is
//! surfaced to the user as a distinct, actionable state rather than a silent
//! error — but it still hands the responses to [`klipper`] for interpretation.

pub mod detection;
pub mod klipper;

#[cfg(not(target_arch = "wasm32"))]
mod transport;

pub use detection::{DetectionFinding, DetectionOption, DetectionQuestion, PrinterDetection};

#[cfg(not(target_arch = "wasm32"))]
pub use transport::{check_status, detect_printer, send_gcode, PrinterStatusReport, SendOutcome};

#[cfg(target_arch = "wasm32")]
mod wasm;
