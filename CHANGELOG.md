# Changelog

All notable changes to Slicer Engine are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

This file is the **single source of truth** for release notes. It is embedded into
every build (CLI, WS server, WASM/UI, desktop) at compile time and republished
verbatim as the body of each GitHub Release. See [RELEASING.md](RELEASING.md) for
the workflow that keeps those in sync.

<!--
Maintainers: keep an `## [Unreleased]` section at the top. When cutting a release,
rename it to `## [x.y.z] - YYYY-MM-DD` and add a fresh empty `## [Unreleased]`
above it. The `release` skill (say "cut a release") automates this — it curates
these notes and acknowledges contributors. `scripts/gen-changelog-draft.sh` and
`scripts/release-contributors.sh` provide the raw material if you do it by hand.
-->

## [Unreleased]

### Added

- **Volumetric-flow limiter** — the `max_volumetric_speed` parameter (mm³/s) is
  now enforced by the G-code generator. On every extruding move the feedrate is
  capped to `max_volumetric_speed · 60 / (layer_height × width)` so the hotend
  is never asked to melt faster than it can, honoured per-segment for
  variable-width Arachne beads and on wall/coasting close moves alike. It stacks
  with the existing wide-bead throttle (the lower speed wins). Defaults to `0`
  (unlimited), so existing output is unchanged.
- **Geometry-aware acceleration** — new `outer_wall_acceleration` and
  `bridge_acceleration` slicing parameters extend layer-type acceleration with
  role-specific limits: a lower outer-wall value cuts ringing on visible
  surfaces, and a low bridge/overhang value keeps flow steady over unsupported
  spans. Precedence is first layer → bridge/overhang → top surface → outer wall
  → normal, each falling back to `acceleration`. Both default to `0` (use the
  normal value), so existing output is unchanged.
- **Layer-type acceleration control** — new `acceleration`, `first_layer_acceleration`,
  and `top_surface_acceleration` slicing parameters. When set, the slicer emits a
  firmware acceleration command whenever the target changes — Klipper
  `SET_VELOCITY_LIMIT ACCEL=…`, Marlin `M204 P…` — using a lower first-layer value
  for adhesion and a distinct top-surface value for finish. All default to `0`
  (disabled), so existing output is unchanged.
- **Pressure / linear advance output** — when a filament or process profile sets
  a non-zero pressure-advance value it is now emitted to G-code once, right after
  the start script. The active firmware dialect renders the correct form
  (Klipper `SET_PRESSURE_ADVANCE ADVANCE=…`, Marlin `M900 K…`). Leaving the value
  at `0` disables it, so existing output is unchanged.
- **G-code metadata header** — every generated program now opens with a
  flavor-specific metadata block (Marlin `HEADER_BLOCK_*`, Klipper
  `KLIPPER_HEADER_*`) carrying the slicer version and timestamp, model name,
  layer count, model height, filament usage (mm / cm³ / g), an estimated print
  time, and the model bounding box. A new `filament_density_g_cm3` setting
  (defaulting to PLA) drives the weight calculation. ([#15](https://github.com/max-scopp/slicer-engine/issues/15))
- **Slice diagnostics & bed-type tracking** — a new `bed_type` setting is
  recorded in the header (`; bed_type:`) for printer integration, and the `slice`
  CLI now reports model height, filament usage, and the estimated print time in
  both its human and JSON output. ([#11](https://github.com/max-scopp/slicer-engine/issues/11))
- **Live versioning** — every build now reports its true version, derived from
  git tags at build time. Local development builds report `development` instead of
  a misleading fixed number.
- **Embedded changelog** — this changelog is bundled into every target and the UI
  shows a "What's New" dialog the first time it runs after an upgrade.
- **GitHub Releases pipeline** — tagging `vX.Y.Z` builds all targets and publishes
  a GitHub Release whose notes are taken from this file (see [RELEASING.md](RELEASING.md)).
- **Commit revision in build info** — every build now records the exact short
  commit hash it was compiled from. The Settings → General page shows it beneath
  the version, the CLI `info` command reports a `Commit:` line (and `git_sha` in
  JSON), so a deployed build can be pinned to a precise source revision instead
  of just an official version number.

### Changed

- **Dependency maintenance** — cleared the outstanding Dependabot backlog. The
  Angular front-end moves to the 22.x line (all `@angular/*` packages, the
  Angular CLI/build toolchain, `@angular/cdk`, and `ngx-markdown`) on
  TypeScript 6.0; the Rust engine moves to `sea-orm` 2.0 and `reqwest` 0.13; and
  the grouped npm minor/patch bumps (three.js, monaco-editor, vitest, prettier,
  fuse.js, iconoir, mermaid, the Tauri CLI, …) are applied. Two proposed bumps
  are intentionally held back and ignored by Dependabot going forward:
  TypeScript 7 (Angular pins `typescript` to `>=6.0 <6.1`) and `getrandom` 0.4
  (declared only to enable the `wasm_js` backend for the 0.3 copy that
  `tobj`/`ahash` still require). No behavioural changes to sliced output.

### Fixed

- **Arachne "splat" gap-fill and gap fill printed under top surfaces** — two
  Benchy-visible quality defects in the Arachne wall generator:
  - Isolated sub-millimetre gap-fill beads (≈ 270 on a 3DBenchy) that added
    nothing but a full retract → travel → un-retract cycle each — wasting time
    and risking filament grinding — are now dropped. The automatic minimum
    gap-fill run length rose from one nozzle diameter to `2·d` (0.8 mm at a
    0.4 mm nozzle), matching the medial-skeleton spur floor. Set
    `gap_fill_min_length_mm` explicitly to override. The residual such short
    beads would have filled is covered by the squish of the flanking wall beads,
    so no wall-zone void opens (verified against the `classic` generator).
  - A redundant gap-fill bead running down the centre of a thin solid strip
    (e.g. the Benchy rear-rail roof, ≈ layer 201) that double-extruded straight
    under the top-surface fill is now pruned **and** the solid surface fills the
    strip in its place. Gap fill sandwiched by solid surface on both sides is
    removed as redundant; crucially the surface no longer carves that bead's
    footprint out of itself (neither via its explicit gap-fill term nor via the
    shared wall-bead footprint, which also lists gap fill), so the roof fills as
    **one** coherent top surface instead of a split ring with a central hole
    that leaked sparse-infill dashes. A bead that merely abuts a surface on one
    side (a genuine thin neck) is still kept and abutted. Cut model-wide
    gap-fill-under-surface double-extrusion from 7.3 → 0.5 mm² with no new voids.
- **Tiny sparse-infill "splat" dashes** — two complementary fixes:
  - `layer.solid_regions` is now **grown by one bead width before being
    subtracted** from the sparse-infill area. The solid surface is printed as a
    stepped serpentine whose extent only approximates its nominal polygon, so
    subtracting the raw outline left a thin crescent sliver hugging the wall
    along every curved perimeter; the scanline shattered it into sub-millimetre
    dashes (31 on one 3DBenchy layer alone), each an isolated dab costing a full
    retract/travel/un-retract for no structural gain — the space is already
    flanked by the solid surface on one side and a wall bead on the other.
    Isolated sub-1.5 mm infill paths on that layer drop 33 → 6. Because the
    correction is keyed to `solid_regions`, it is an exact **no-op on layers
    with no solid surface**, so genuinely thin wall-to-wall cavities (hollow-box
    lattices) keep their infill untouched.
  - `min_infill_extrusion_mm` (default 0.4 mm) now also filters *sparse* infill,
    not just solid surface fill, catching the residual sub-threshold segments a
    legitimate region's tapering corners produce.

  Together with the gap-fill fixes this cut isolated sub-0.8 mm extrusions on the
  3DBenchy by ~76 % (356 → 87) with no change to strength (the flanking walls and
  solid surface fill the space).

- **iPad Apple Pencil + two-finger navigation "spazzing"** — palm rejection
  classified each touch on its own, so a stylus user's two-finger pan/pinch could
  lose exactly one finger — a firm fingertip read as palm-sized, or a flickering
  pen hover/grace state — collapsing the gesture into an unwanted single-finger
  camera rotate. The viewport's pointer arbiter now decides **per gesture
  group**: the first contact is classified, and any finger that lands while
  another is already down inherits that verdict, so a two-finger gesture is
  admitted or rejected as a whole and never split. The palm-by-size heuristic is
  also gated to _recent_ pen use instead of the whole session, so firm fingertips
  stop being mistaken for a palm long after the pencil is set down. Stale
  pointer state is now reclaimed by timeout, so a touch or pen event dropped by
  the OS (a common iPad backgrounding glitch) can no longer wedge the viewport
  into ignoring all touch until reload.

- **3MF models loaded at the wrong scale** — the 3MF importer ignored the
  `<model unit="…">` declaration and read every coordinate as raw millimeters, so
  files authored in `meter`, `centimeter`, `inch`, or `foot` opened dramatically
  undersized (e.g. a metre-declared object appeared 1000× too small). The loader
  now normalizes all six spec units (`micron`, `millimeter`, `centimeter`,
  `inch`, `foot`, `meter`) to millimeters on import and rejects unrecognized
  units with a clear error. Applies to both the CLI file path and the
  byte-upload path used by the UI/WASM and WS server.

## [0.1.0] - 2026-08-23

### Added

- Initial slicer engine: STL/OBJ/3MF loading, mesh slicing, Arachne
  variable-width wall generation, top/bottom surface detection, and infill.
- Unified scene engine (single source of truth for object placement) shared by
  the CLI, WebSocket server, and WASM UI.
- Angular UI with a Three.js viewport and G-code preview, plus a Tauri desktop
  shell.
- Command-line interface with `slice`, `info`, and schema-generation commands.
