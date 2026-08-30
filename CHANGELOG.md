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

Style: keep each entry to one to three tight lines (what it does, its default,
the one number worth quoting) — deep rationale lives in AGENTS.md and the module
READMEs, not here. Break a long category into `#### Theme` groups so it stays
scannable, keep the bold **Feature name** lead on every bullet, and never put
issue/PR numbers or repo links in the notes. See the tone rules in
.github/skills/release/SKILL.md for the full voice.
-->

## [Unreleased]

### Added

- **Undo / redo buttons in the 3D view** — forward and backward history buttons
  on the plate toolbar for touch devices where the ⌘/Ctrl+Z shortcut can't be
  reached. Shown automatically on keyboard-less tablets and phones; force them on
  or off in Settings → General → Controls. Auto by default.

### Fixed

- **Undo no longer wipes the plate after navigating** — dip into Settings and
  back, or use the browser's back button, and your objects keep their positions
  and undo history instead of being deleted by the next undo. History now resets
  only when the workplate is genuinely replaced.
### Fixed

- **The "re-slice" hint no longer clears itself** — moving an object or changing
  a setting *while a slice is running* used to be silently absorbed into the
  preview once it finished, so the "Scene changed — re-slice" hint disappeared
  even though the on-screen G-code predated your edit. The comparison baseline is
  now captured the moment you press Slice, so a mid-slice change correctly keeps
  the hint lit until you re-slice.

- **No more flashing console on Windows** — the desktop app used to re-read the
  OS accent colour every couple of seconds by shelling out, which popped a brief
  `cmd` window on Windows on every check. It now reads the accent directly and
  waits for the OS to signal a change instead of polling, so the flickering after
  login is gone and macOS stops re-checking on a timer too.

- **No more launch hang on Windows** — the desktop app used to open a blank,
  unresponsive window for a moment on Windows before it became usable. Its window
  now stays hidden until the interface has actually drawn, so the app appears
  fully rendered and ready the instant you see it.

### Changed

- **Live accent tracking is event-driven** — the desktop app now updates its
  accent tint the moment you change it in the OS (Windows registry change
  notifications; macOS distributed notifications) rather than catching up on a
  2-second poll.

## [0.2.0] - 2026-08-30

The build plate grows up. What used to be one model on a plate is now a real
build plate — add as many objects as you like, cancel a failed one mid-print, or
print them one at a time — and every infill pattern finally deposits the exact
density you ask for at any line width.

### Highlights

- **Multi-object build plates** — place several models on one plate, manage them
  in a new Objects panel, cancel a single failed part from Mainsail/Fluidd, or
  print parts sequentially front-to-back. Multi-part 3MFs land as separate, named
  objects instead of one fused blob.
- **Infill that hits its density** — line spacing and per-line flow now come from
  the real extrusion width, not a hardcoded 0.4 mm. Grid stopped printing double,
  honeycomb tiles and stacks like it should, and a 0.6 mm nozzle asked for 20 %
  no longer prints ~13 %.

### Added

#### Infill & surfaces

- **Advanced infill options** — infill anchors (weld sparse lines to the wall and
  merge broken dashes into one continuous move — the biggest quality win here),
  sparse-infill layer combining (print shared infill once at a stacked height),
  internal solid layers, separate top/bottom/internal surface patterns (including
  monotonic for a cleaner top finish), and a bridging-angle override. All off, or
  matching the previous behaviour, by default.
- **Five more infill patterns** — `aligned-rectilinear`, `triangles`,
  `tri-hexagon`, `cubic` and `concentric`. OrcaSlicer's pattern names are accepted
  as-is, so imported profiles map without a translation table.
- **Spiral (vase) mode** — `spiral_vase` (CLI `--spiral-vase`) prints a single
  continuous outer wall whose Z ramps smoothly over each layer, for a seamless
  single-wall vase with no Z-seam. Off by default.

#### Multi-object build plates

- **Cancel one object mid-print, or print objects one at a time** — the plate now
  tracks which part every extrusion belongs to. *Exclude object* wraps each part
  in firmware markers (Klipper `EXCLUDE_OBJECT_*`, Marlin / RepRapFirmware `M486`)
  so a failed part can be cancelled from Mainsail, Fluidd or OctoPrint while the
  rest of the plate carries on. *Sequential printing* (Print order → by object)
  finishes each part front-to-back, lifting clear of everything already on the
  bed, with clearance checks reported as warnings and optional between-object
  G-code. Both off by default; with both off the plate slices exactly as before.
- **Multiple objects per workplate** — a plate is now a build plate, not a single
  file. An **Add model** button and a multi-select picker place more models
  without replacing what's there, a new **Objects panel** lists and manages them
  (flagging anything out of bounds or overlapping), and reopening a saved plate
  restores every object. Multi-part 3MF files now land as separate, named objects
  instead of one fused blob.
- **One placement command** — a single **Place objects** tool replaces the rival
  "auto-orient" and "arrange all" buttons that undid each other's work. It sits
  with move / rotate / scale and opens a card for the two things worth varying
  (auto-orient and gap). The same settings apply when you drop a model in.
- **Preferred print orientation, per printer** — set a diagonal angle (e.g. `45°`
  for CoreXY) applied to every auto-oriented part after it's laid on its best
  face. Defaults to `0°` (untouched).
- **Multi-object CLI slicing** — `slice -i part_a.stl -i part_b.stl` builds one
  plate. A new `--arrange` flag (with `--arrange-spacing` and
  `--arrange-auto-orient`) packs it without overlap, and the transform flags apply
  to every loaded model.

#### Printer & firmware output

- **Chamber temperature management** — the filament asks for a chamber temperature
  (with a hotter first-layer soak) and the printer says whether it can deliver it;
  only when both agree are directives emitted, with a safe soak sequence and
  Klipper's native commands. A start G-code that already heats the chamber keeps
  ownership.
- **Z offset** — a per-printer `z_offset_mm` (Settings → Printers → Hardware)
  added to every Z coordinate written to the G-code, so it works on any firmware
  with no macro to maintain. Same meaning as in PrusaSlicer and OrcaSlicer.
- **Dynamic overhang speed & cooling** — perimeter segments are graded by how much
  hangs over unsupported air, and each degree prints at its own speed with extra
  part-cooling. On by default.
- **Advanced retraction modes** — firmware retraction (`G10`/`G11`), relative
  extruder distances, minimum-travel-before-retract, restart-extra prime,
  retract-on-layer-change, and wipe-while-retracting. All default to the previous
  behaviour.
- **Perimeter routing & ordering options** — `external_perimeters_first` (outer
  wall now printed last by default, matching PrusaSlicer/Orca/Cura),
  `extra_perimeters`, `thin_walls` (classic generator), and
  `ensure_vertical_shell_thickness` and `avoid_crossing_perimeters`. Defaults
  preserve existing behaviour except the inner-first ordering.
- **Volumetric-flow limiter** — `max_volumetric_speed` (mm³/s) caps the feedrate
  so the hotend is never asked to melt faster than it can, per-segment for
  variable-width beads. Defaults to `0` (unlimited).
- **Geometry-aware acceleration** — `outer_wall_acceleration` and
  `bridge_acceleration` add role-specific limits on top of layer-type
  acceleration (lower outer-wall for less ringing, low bridge for steady flow).
  Default `0`.
- **Layer-type acceleration control** — `acceleration`, `first_layer_acceleration`
  and `top_surface_acceleration` emit a firmware acceleration command when the
  target changes. Default `0`.
- **Pressure / linear advance output** — a non-zero pressure-advance value is
  emitted once after the start script in the correct firmware form (Klipper
  `SET_PRESSURE_ADVANCE`, Marlin `M900 K`). `0` disables it.
- **G-code metadata header** — every program opens with a flavor-specific metadata
  block: slicer version and timestamp, model name, layer count, height, filament
  usage, time estimate and bounding box. A new `filament_density_g_cm3` drives the
  weight.

#### Models & profiles

- **Automatic mesh repair on import** — holes are capped, cracked vertices welded,
  inside-out triangles turned right-side-out, and zero-area or duplicate triangles
  dropped. The UI raises a toast, the CLI logs a warning, and a new
  `mesh-check` command prints a full report without slicing. Clean models are
  never touched; pass `--no-mesh-repair` to slice the raw geometry.
- **Export your profile library** — Settings → General downloads every printer,
  filament, print profile and label as TOML (a ZIP bundle, or a single
  `profiles.toml` the engine reads directly). Printer API keys are stripped, so
  the export is safe to share.
- **Settings tell you when they depend on something else** — a chamber temperature
  set for a printer with no chamber heater now says plainly it won't take effect
  and links to the fix. The CLI prints these warnings too.

#### App, platform & tooling

- **Release notes inside the app** — Settings → What's New lists every release,
  newest first, with the version you're running highlighted. The post-upgrade
  dialog shows that same list.
- **iPadOS / iOS target** — the Tauri shell now builds and runs on iPad with the
  full slicing engine on-device. The `pnpm run ios:*` scripts drive the toolchain,
  Xcode project generation, and a live-reload simulator build.
- **Slice diagnostics & bed-type tracking** — a `bed_type` setting is recorded in
  the header, and the `slice` CLI reports model height, filament usage and the
  estimated print time.
- **Live versioning** — every build reports its true version, derived from git
  tags. Development builds report `development`.
- **Embedded changelog** — bundled into every target; the UI shows a "What's New"
  dialog the first time it runs after an upgrade.
- **Releases pipeline** — tagging `vX.Y.Z` builds all targets and publishes a
  release whose notes are taken from this file.
- **Commit revision in build info** — every build records the exact short commit
  hash, shown in Settings → General and reported by the CLI `info` command.

### Changed

#### Infill accuracy & patterns

- **Infill density is now accurate at every line width** — line spacing and the
  flow charged for each line both come from the real extrusion width, not a
  hardcoded 0.4 mm reference. A 0.6 mm nozzle asked for 20 % used to print ~13 %.
- **Grid infill no longer prints double** — it laid two full-density passes
  instead of two half-density ones. Honeycomb cells and the gyroid period are on
  the libslic3r relations now too, and all scale with the line width.
- **Honeycomb is a real hexagonal tiling** — continuous zig-zag walls drawn once,
  instead of stamped hexagons with every shared wall drawn twice.
- **Honeycomb cells stack again** — it (and triangles, tri-hexagon, cubic) was
  rotated 90° every other layer and keyed to the region's bounding box, so walls
  landed on the layer below's voids. Consecutive Voron-cube layers now share 79 %
  of their infill geometry, up from 2 %.
- **TPMS-D actually prints now** — its segments are chained into continuous curves
  and the period recalibrated, so it deposits the full requested density instead
  of about a seventh.
- **Top surfaces default to monotonic line, bottoms to monotonic** — a cleaner,
  direction-consistent finish that also stopped 106 mm² of top-surface material
  printing over the inner wall on a Voron cube.

#### Docs & dependencies

- **The documentation now leads with the product, not the architecture** — a
  proper guide to *using* Cold Crabby plus a teams track for self-hosting and
  configuration, with the engineering docs slimmed to a map. A banner notes the
  docs are early and their structure may still change.
- **Dependency maintenance** — cleared the Dependabot backlog: the Angular
  front-end moves to the 22.x line on TypeScript 6.0, the Rust engine to
  `sea-orm` 2.0 and `reqwest` 0.13, plus the grouped npm bumps. No behavioural
  change to sliced output.

### Fixed

#### Print quality

- **Your filament's cooling settings are now actually used** — Fan Speed, Bridge
  Fan Speed, First Layer Fan Speed and Fan Off For First Layers were shown and
  saved but ignored by the generator, so the part-cooling fan ran during the first
  layer for every material, quietly costing bed adhesion. Fan Speed is now the
  cooling ceiling, and the bridge and overhang boosts are held back on the layers
  where cooling is off.
- **Isolated infill specks in narrow wedges** — a connected infill region too
  small to hold more than one dash (2 mm² at a 0.4 mm nozzle) is now skipped. It's
  an **area** rule, so a genuinely thin cavity that deserves a lattice keeps every
  line.
- **Generator-specific wall options are now hidden for the generator that ignores
  them** — `thin_walls` / `wall_distribution_count` (classic) and
  `gap_fill_min_length_mm` (Arachne). This also stopped `thin_walls` from silently
  deleting Arachne's thin features — turning it off had wiped ~50 slot fins on a
  filament card caddy. Arachne now always prints thin features.
- **Top-surface "squiggles" where solid fill grazes a wall** — a surface meeting
  the wall band at a shallow angle was filled with a dense micro-serpentine of
  sub-millimetre stubs. Solid surface regions narrower than one extrusion width
  are now dropped before filling, while thicker geometry keeps its exact shape and
  sharp corners.
- **Arachne "splat" gap-fill and gap fill under top surfaces** — isolated
  sub-millimetre gap-fill beads (≈270 on a 3DBenchy) that cost a full
  retract/travel/un-retract each are dropped, and a redundant bead running under a
  thin solid strip is pruned with the surface filling the strip in its place.
- **Tiny sparse-infill "splat" dashes** — `solid_regions` is now grown by one bead
  before being subtracted from the infill area (killing the crescent slivers the
  scanline shattered into dashes), and `min_infill_extrusion_mm` also filters
  sparse infill. Together with the gap-fill fixes this cut isolated sub-0.8 mm
  extrusions on a 3DBenchy by ~76 %.

#### Interface & import

- **The documentation site failed to build** — a line of prose wrapped an inline
  code span across a line break, leaking its placeholders out as raw, unclosed
  HTML.
- **The transform panel was blank whenever more than one object was selected** —
  it now edits the whole selection: **Position** shifts every part by the same
  amount (keeping your layout), while **Rotation**, **Scale** and **Size** apply
  per part about each one's own centre.
- **The arrange gap defaulted to 0 mm on a fresh install** — an unset preference
  read back as `0` rather than "unset". It now starts at 4 mm.
- **iPad Apple Pencil + two-finger navigation "spazzing"** — the viewport's
  pointer arbiter now classifies a whole gesture group at once, so a two-finger
  pan/pinch is never split into a stray single-finger camera rotate.
- **3MF models loaded at the wrong scale** — the importer ignored the
  `<model unit="…">` declaration and read every coordinate as millimeters. All six
  spec units are now normalized on import.
- **Viewport-cube ortho snap popped back to perspective on pan/zoom** — only a
  genuine rotate now breaks the flattened, dimension-true view; panning and
  zooming keep it, so you can inspect a snapped view up close.

### Contributors

Thanks to @max-scopp, who built everything in this release. Onward to 0.2.0.

## [0.1.0] - 2026-08-23

### Added

- Initial slicer engine: STL/OBJ/3MF loading, mesh slicing, Arachne
  variable-width wall generation, top/bottom surface detection, and infill.
- Unified scene engine (single source of truth for object placement) shared by
  the CLI, WebSocket server, and WASM UI.
- Angular UI with a Three.js viewport and G-code preview, plus a Tauri desktop
  shell.
- Command-line interface with `slice`, `info`, and schema-generation commands.
