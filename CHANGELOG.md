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

## [0.1.0] - 2026-08-23

### Added

- Initial slicer engine: STL/OBJ/3MF loading, mesh slicing, Arachne
  variable-width wall generation, top/bottom surface detection, and infill.
- Unified scene engine (single source of truth for object placement) shared by
  the CLI, WebSocket server, and WASM UI.
- Angular UI with a Three.js viewport and G-code preview, plus a Tauri desktop
  shell.
- Command-line interface with `slice`, `info`, and schema-generation commands.
