# `logging` — Process Logger & Phase Performance Markers

Provides the [`ProcessLogger`] trait and all standard implementations used
across the slicing pipeline (CLI, WebSocket, and test utilities).

---

## Module layout

```
logging.rs          ProcessLogger trait, PhaseTimer, phase constants,
                    StderrLogger, NullLogger
cli/emit.rs         CliLogger  — routes through Emitter (JSON / human stderr)
server/ws_session.rs WsLogger  — mirrors to stderr + WebSocket PhaseMarker frames
```

---

## Log levels

| Method | Visibility | Typical use |
|---|---|---|
| `log_info` | Always visible | Layer count, completion messages |
| `log_debug` | Suppressed when not verbose | AABB, face counts, step details |
| `log_warn` | Always visible | Non-fatal anomalies (dialect fallbacks, etc.) |
| `log_phase_start` | Always visible | Start of a named pipeline phase |
| `log_phase_end` | Always visible | End of a phase with elapsed ms |

---

## Phase performance markers

Each major pipeline step is bracketed by `log_phase_start` / `log_phase_end`
calls. The elapsed wall-clock time (in milliseconds) is reported by the end
marker. Use [`PhaseTimer`] to automate start/end pairing:

```rust
use slicer_engine::logging::{PhaseTimer, phases};

let t = PhaseTimer::start(phases::SLICING, &logger);
// … do work …
t.finish(); // calls log_phase_end with measured elapsed_ms
```

### Standard phase names

All phases are defined as constants in the [`phases`] submodule to ensure
consistent names across every logger backend.

| Constant | Phase | Instrumented in |
|---|---|---|
| `phases::TOTAL` | Complete end-to-end slicing process | CLI `slice.rs`, WebSocket handler (wraps all other phases) |
| `phases::MESH_LOAD` | STL file read + mesh parse | CLI `slice.rs`, WebSocket handler |
| `phases::MESH_ANALYSIS` | AABB / volume / surface-area computation | CLI `slice.rs` (verbose) |
| `phases::SLICING` | Triangle–plane intersection → layers | `core::process_mesh` |
| `phases::SURFACES` | Top/bottom solid-surface generation | `core::process_mesh` |
| `phases::INFILL` | Sparse and solid infill generation | `core::process_mesh` (future) |
| `phases::GCODE_GENERATION` | G-code program construction | CLI `slice.rs`, WebSocket handler |
| `phases::FILE_WRITE` | Writing G-code file to disk | CLI `slice.rs`, WebSocket handler |

### After-slice overview

A complete slice run produces phase markers in this logical order:

```
[phase] total            → start
[phase] mesh_load        → start
[phase] mesh_load        ✓ 12 ms
[phase] mesh_analysis    → start      (CLI verbose only)
[phase] mesh_analysis    ✓ 0 ms
[phase] slicing          → start
[phase] slicing          ✓ 340 ms
[phase] surfaces         → start
[phase] surfaces         ✓ 85 ms
[phase] gcode_generation → start
[phase] gcode_generation ✓ 22 ms
[phase] file_write       → start
[phase] file_write       ✓ 5 ms
[phase] total            ✓ 464 ms
```

The `total` phase spans all others and reports the complete end-to-end time.
Individual phase timings identify which pipeline step dominates total
print-preparation time, making it straightforward to spot regressions or
optimisation targets.

---

## Logger backends

### `StderrLogger` (global)

Writes every event unconditionally to **stderr**. Operators can `tail -f` the
server log and see all activity regardless of which interface (CLI or WS)
triggered the run.

Phase output format:
```
[phase] <phase> → start
[phase] <phase> ✓ <ms> ms
```

### `CliLogger` (CLI)

Wraps [`Emitter`] and gates `log_debug` on the `--verbose` flag. Phase markers
are always emitted.

- **Human mode** (default): same `[phase]` prefix as `StderrLogger`.
- **JSON mode** (`--output-format json`): structured JSON to stderr:
  ```json
  {"$schema":"slicer-engine/log-v1","level":"phase_start","phase":"slicing"}
  {"$schema":"slicer-engine/log-v1","level":"phase_end","phase":"slicing","elapsed_ms":340}
  ```

### `WsLogger` (WebSocket)

Mirrors every event to `StderrLogger` **and** sends a typed `PhaseMarker`
frame back to the browser client:

```json
{"type":"PhaseMarker","phase":"slicing","event":"start"}
{"type":"PhaseMarker","phase":"slicing","event":"end","elapsed_ms":340}
```

The browser's status panel can display a live timing table as the pipeline
progresses.

### `NullLogger` (tests)

Silently discards all events. Use when log output would clutter test output.

---

## Adding a new phase

1. Add a constant to the `phases` submodule in `src/logging.rs`.
2. Wrap the new code with `PhaseTimer::start(phases::MY_PHASE, logger)` / `.finish()`.
3. Update the table in this README.

---

## See also

- [`src/core/pipeline.rs`](core/pipeline.rs) — `process_mesh` pipeline entry point
- [`src/cli/commands/slice.rs`](cli/commands/slice.rs) — CLI phase instrumentation
- [`src/server/ws_session.rs`](server/ws_session.rs) — WebSocket phase instrumentation
- [`src/ws_protocol.rs`](ws_protocol.rs) — `ServerMessage::PhaseMarker` wire type
