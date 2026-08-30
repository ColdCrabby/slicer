//! Shared logging infrastructure for the slicing pipeline.
//!
//! The [`ProcessLogger`] trait provides a uniform logging interface that all
//! pipeline entry points (CLI, WebSocket, etc.) share. There are two flavours:
//!
//! - **Global logger** ([`StderrLogger`]): always writes to stderr. Operators
//!   can tail server logs in one place regardless of which interface triggered
//!   the slicing run.
//!
//! - **Request-specific loggers** (in their respective modules):
//!   - `cli::emit::CliLogger` – routes through the CLI [`Emitter`], gating
//!     debug output on the `--verbose` flag. Because the emitter writes to
//!     stderr it naturally mirrors to the global logger.
//!   - `server::ws_session::WsLogger` – relays every message to the global
//!     stderr logger **and** sends a JSON log frame back to the WebSocket
//!     client, giving the browser the same verbosity the CLI enjoys.
//!
//! ## Performance Timing Markers
//!
//! Each logger also supports **phase timing markers** via [`ProcessLogger::log_phase_start`]
//! and [`ProcessLogger::log_phase_end`]. These measure wall-clock elapsed time for
//! named pipeline phases and surface the results through the same logger backends.
//!
//! Use [`PhaseTimer`] for RAII-style timing that automatically records start/end events:
//!
//! ```rust
//! use slicer_engine::logging::{NullLogger, PhaseTimer, phases};
//!
//! let logger = NullLogger;
//! let t = PhaseTimer::start(phases::SLICING, &logger);
//! // … do work …
//! t.finish();
//! ```
//!
//! Standard phase names are defined in the [`phases`] submodule.
//!
//! [`Emitter`]: crate::cli::emit::Emitter

/// Standard phase name constants for the slicing pipeline.
///
/// Use these constants wherever a phase name is required to ensure consistency
/// across CLI, WebSocket, and any future logger backends.
///
/// # Phases
///
/// | Constant | Phase |
/// |---|---|
/// | [`TOTAL`] | Complete end-to-end slicing process |
/// | [`MESH_LOAD`] | STL file read + mesh parsing |
/// | [`MESH_ANALYSIS`] | AABB / volume / surface-area computation |
/// | [`SLICING`] | Triangle–plane intersection → `Vec<SliceLayer>` |
/// | [`WALL_GENERATION`] | Perimeter (wall) bead generation |
/// | [`INFILL_REGION_SNAPSHOT`] | Pre-strip infill interior region snapshot |
/// | [`WALL_RESTRICTIONS`] | Single-wall first/top-layer restriction pass |
/// | [`INTERIOR_REGIONS`] | Per-layer interior region computation for surfaces |
/// | [`SURFACES`] | Top/bottom solid-surface generation |
/// | [`INFILL`] | Sparse and solid infill pattern generation |
/// | [`GCODE_GENERATION`] | G-code program construction |
/// | [`FILE_WRITE`] | Writing the G-code file to disk |
pub mod phases {
    /// Complete end-to-end slicing process (spans all other phases).
    pub const TOTAL: &str = "total";
    /// STL file read and mesh parsing phase.
    pub const MESH_LOAD: &str = "mesh_load";
    /// AABB, volume, and surface-area computation phase.
    pub const MESH_ANALYSIS: &str = "mesh_analysis";
    /// Triangle–plane intersection and layer contour extraction phase.
    pub const SLICING: &str = "slicing";
    /// Perimeter (wall) bead generation (classic or Arachne generator).
    pub const WALL_GENERATION: &str = "wall_generation";
    /// Snapshot of infill interior regions taken before wall stripping.
    pub const INFILL_REGION_SNAPSHOT: &str = "infill_region_snapshot";
    /// Single-wall restriction pass for first and top layers.
    pub const WALL_RESTRICTIONS: &str = "wall_restrictions";
    /// Per-layer interior region computation (boundary for surfaces and infill).
    pub const INTERIOR_REGIONS: &str = "interior_regions";
    /// Top-surface geometry detection pass inside the wall-restriction step.
    pub const WALL_TOP_DETECT: &str = "wall_top_detect";
    /// Inner-wall stripping pass of the single-wall restriction step.
    pub const WALL_APPLY: &str = "wall_apply";
    /// Top/bottom solid-surface generation phase.
    pub const SURFACES: &str = "surfaces";
    /// Sparse and solid infill pattern generation phase.
    pub const INFILL: &str = "infill";
    /// G-code program construction phase.
    pub const GCODE_GENERATION: &str = "gcode_generation";
    /// Writing the G-code output file to disk phase.
    pub const FILE_WRITE: &str = "file_write";
}

/// Unified logging interface for the slicing pipeline.
///
/// Every implementation must:
/// 1. Write to the **global** stderr sink so server operators see all activity.
/// 2. Optionally relay messages to a request-specific sink (e.g. a WebSocket
///    connection or a CLI emitter with format-aware rendering).
///
/// The three levels map to familiar semantics:
/// - `log_info`  – important results that should always surface (layer count, etc.)
/// - `log_debug` – verbose step details; may be suppressed in non-verbose contexts
/// - `log_warn`  – non-fatal issues the operator should be aware of
///
/// Phase timing is reported through [`log_phase_start`] / [`log_phase_end`]. Both
/// have default no-op implementations so existing loggers remain valid without
/// any changes.
///
/// [`log_phase_start`]: ProcessLogger::log_phase_start
/// [`log_phase_end`]: ProcessLogger::log_phase_end
pub trait ProcessLogger: Send + Sync {
    /// Emit an informational message that is always visible.
    fn log_info(&self, msg: &str);

    /// Emit a debug message that may be suppressed in non-verbose contexts.
    fn log_debug(&self, msg: &str);

    /// Emit a warning message that is always visible.
    fn log_warn(&self, msg: &str);

    /// Emit a phase-start timing marker.
    ///
    /// Called at the beginning of a named pipeline phase (see [`phases`]).
    /// The default implementation is a no-op, preserving backward compatibility.
    fn log_phase_start(&self, _phase: &str) {}

    /// Emit a phase-end timing marker with elapsed wall-clock time in milliseconds.
    ///
    /// Called at the end of a named pipeline phase (see [`phases`]).
    /// The default implementation is a no-op, preserving backward compatibility.
    fn log_phase_end(&self, _phase: &str, _elapsed_ms: u64) {}

    /// Announce that the phases which follow belong to object `index` of
    /// `count` on a multi-object plate (both **1-based**).
    ///
    /// A plate sliced object-by-object runs the whole pipeline once per object,
    /// so the same phase names recur. Without this context a progress bar keyed
    /// on phase names walks *backwards* every time a new object restarts the
    /// pipeline. Loggers that surface progress record the scope and stamp it
    /// onto every subsequent phase marker until [`clear_object_scope`] is
    /// called. The default is a no-op.
    ///
    /// [`clear_object_scope`]: ProcessLogger::clear_object_scope
    fn set_object_scope(&self, _index: usize, _count: usize) {}

    /// Clear the object scope set by [`set_object_scope`], so later phases are
    /// reported without object context again. The default is a no-op.
    ///
    /// [`set_object_scope`]: ProcessLogger::set_object_scope
    fn clear_object_scope(&self) {}

    /// Returns `true` when the pipeline should abort early.
    ///
    /// Checked between major phases in [`crate::core::process_mesh`]. The
    /// default always returns `false`, so existing loggers remain valid
    /// without any changes.
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// RAII guard that records a [`ProcessLogger::log_phase_start`] event on creation
/// and a [`ProcessLogger::log_phase_end`] event (with measured elapsed time) when
/// [`PhaseTimer::finish`] is called.
///
/// # Example
///
/// ```rust
/// use slicer_engine::logging::{NullLogger, PhaseTimer, phases};
///
/// let logger = NullLogger;
/// let t = PhaseTimer::start(phases::GCODE_GENERATION, &logger);
/// // … generate G-code …
/// t.finish();
/// ```
pub struct PhaseTimer<'a> {
    phase: &'a str,
    logger: &'a dyn ProcessLogger,
    // std::time::Instant is not available on wasm32-unknown-unknown.
    #[cfg(not(target_arch = "wasm32"))]
    start: std::time::Instant,
    #[cfg(target_arch = "wasm32")]
    start_ms: f64,
}

impl<'a> PhaseTimer<'a> {
    /// Begin timing a phase and emit a start marker.
    pub fn start(phase: &'a str, logger: &'a dyn ProcessLogger) -> Self {
        logger.log_phase_start(phase);
        Self {
            phase,
            logger,
            #[cfg(not(target_arch = "wasm32"))]
            start: std::time::Instant::now(),
            #[cfg(target_arch = "wasm32")]
            start_ms: js_sys::Date::now(),
        }
    }

    /// Stop timing and emit an end marker with the measured elapsed time.
    pub fn finish(self) {
        #[cfg(not(target_arch = "wasm32"))]
        let elapsed_ms = self.start.elapsed().as_millis() as u64;
        #[cfg(target_arch = "wasm32")]
        let elapsed_ms = (js_sys::Date::now() - self.start_ms).max(0.0).round() as u64;
        self.logger.log_phase_end(self.phase, elapsed_ms);
    }
}

/// Global stderr logger.
///
/// Writes all messages unconditionally to **stderr**. This is the baseline
/// sink that every request-specific logger should also forward to, ensuring
/// that server operators can observe all slicing activity in one stream.
pub struct StderrLogger;

impl ProcessLogger for StderrLogger {
    fn log_info(&self, msg: &str) {
        eprintln!("[info] {}", msg);
    }

    fn log_debug(&self, msg: &str) {
        eprintln!("[debug] {}", msg);
    }

    fn log_warn(&self, msg: &str) {
        eprintln!("\x1b[33m[warn]\x1b[0m {}", msg);
    }

    fn log_phase_start(&self, phase: &str) {
        eprintln!("[phase] {} → start", phase);
    }

    fn log_phase_end(&self, phase: &str, elapsed_ms: u64) {
        eprintln!("[phase] {} ✓ {} ms", phase, elapsed_ms);
    }
}

/// No-op logger that silently discards all messages.
///
/// Intended for unit tests where pipeline log output would be distracting or
/// irrelevant to the assertion being tested.
///
/// # Example
/// ```
/// use slicer_engine::logging::{NullLogger, ProcessLogger};
///
/// let logger = NullLogger;
/// logger.log_info("this goes nowhere");
/// logger.log_debug("also discarded");
/// logger.log_warn("silenced");
/// ```
pub struct NullLogger;

impl ProcessLogger for NullLogger {
    fn log_info(&self, _msg: &str) {}
    fn log_debug(&self, _msg: &str) {}
    fn log_warn(&self, _msg: &str) {}
    fn log_phase_start(&self, _phase: &str) {}
    fn log_phase_end(&self, _phase: &str, _elapsed_ms: u64) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_logger_does_not_panic() {
        let l = NullLogger;
        l.log_info("info");
        l.log_debug("debug");
        l.log_warn("warn");
        l.log_phase_start("slicing");
        l.log_phase_end("slicing", 42);
    }

    #[test]
    fn stderr_logger_does_not_panic() {
        // StderrLogger writes to stderr; we just verify it doesn't panic.
        let l = StderrLogger;
        l.log_info("info");
        l.log_debug("debug");
        l.log_warn("warn");
        l.log_phase_start("slicing");
        l.log_phase_end("slicing", 42);
    }

    #[test]
    fn phase_timer_runs_without_panic() {
        let l = NullLogger;
        let t = PhaseTimer::start(phases::SLICING, &l);
        t.finish();
    }

    #[test]
    fn phase_timer_records_nonzero_or_zero_elapsed() {
        // We can't assert a specific elapsed time, but we can verify the
        // timer calls log_phase_end with the elapsed value by using a logger
        // that captures calls.
        use std::sync::{Arc, Mutex};

        struct CapturingLogger {
            end_calls: Arc<Mutex<Vec<(String, u64)>>>,
        }
        impl ProcessLogger for CapturingLogger {
            fn log_info(&self, _msg: &str) {}
            fn log_debug(&self, _msg: &str) {}
            fn log_warn(&self, _msg: &str) {}
            fn log_phase_end(&self, phase: &str, elapsed_ms: u64) {
                self.end_calls
                    .lock()
                    .unwrap()
                    .push((phase.to_string(), elapsed_ms));
            }
        }

        let calls = Arc::new(Mutex::new(Vec::new()));
        let logger = CapturingLogger {
            end_calls: calls.clone(),
        };
        let t = PhaseTimer::start(phases::SLICING, &logger);
        t.finish();

        let recorded = calls.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, phases::SLICING);
    }

    #[test]
    fn all_phase_constants_are_non_empty() {
        for p in [
            phases::TOTAL,
            phases::MESH_LOAD,
            phases::MESH_ANALYSIS,
            phases::SLICING,
            phases::SURFACES,
            phases::INFILL,
            phases::GCODE_GENERATION,
            phases::FILE_WRITE,
        ] {
            assert!(!p.is_empty(), "phase constant must not be empty");
        }
    }
}
