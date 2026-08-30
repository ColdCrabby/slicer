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

- **Chamber temperature management** — an enclosed printer can now actually heat
  its chamber. The filament says how warm it wants the chamber (`chamber_temp`,
  plus a hotter first-layer soak via `chamber_temp_first_layer`) and the printer
  says whether it can deliver it (`heated_chamber`); only when both agree does
  the slicer emit real directives. The soak runs before your start G-code, with
  the bed armed first — on most enclosures the bed *is* what heats the chamber,
  so waiting on a cold bed would never finish — and with the nozzle still cold,
  so molten filament is never parked in a hot end for the length of a soak. The
  chamber drops back to its steady-state target once the first layer is down.
  Klipper gets its native `SET_HEATER_TEMPERATURE` / `TEMPERATURE_WAIT` pair
  instead of `M141`/`M191`, which it has no built-in support for. A start G-code
  that already heats the chamber (`START_PRINT … CHAMBER={chamber_temp}` and
  friends) keeps full ownership — the slicer stands down rather than heating and
  soaking twice.
  ([#8](https://github.com/max-scopp/slicer-engine/issues/8))
- **Settings tell you when they depend on something else.** A filament setting
  can need a machine capability that the *printer* profile has to provide — and
  until now nothing said so, on a tab where you could not see it. A chamber
  temperature set for a printer that has not been told it has a chamber heater
  now says plainly what will happen ("no chamber command will be emitted"), and
  links straight to the switch that fixes it. The engine reports the same thing,
  so the CLI and the slicer log are equally honest — and the CLI now prints
  every "this setting will not take effect" warning, which it had been computing
  for other runtimes but never showing itself.
  ([#8](https://github.com/max-scopp/slicer-engine/issues/8))

- **Export your profile library** — Settings → General now has a **Backup &
  Export** section that downloads every printer, filament, print profile and
  label as TOML. The default export is a ZIP bundle with one file per profile
  (`printers/01-voron-24.toml`, …); the dropdown offers a single `profiles.toml`
  instead — the exact file the engine and CLI read, so it can be dropped into a
  config directory as-is. Concatenating a bundle reproduces that same file, in
  the original order. The engine renders the export in every runtime (server,
  desktop, and in-browser), so the files always match what the slicer reads
  back. Printer API keys are stripped from the export, so it is safe to share or
  commit — re-enter them after restoring.
- **Dynamic overhang speed & cooling** — perimeter segments are graded by how
  much of the extrusion width hangs over unsupported air, and each degree prints
  at its own speed with extra part-cooling airflow. Enabled by default, tuned for
  the 0–25 / 25–50 / 50–75 / 75–100% unsupported bands via `overhang_1_4_speed`…
  `overhang_4_4_speed`, `overhang_fan_speed`, `overhang_fan_threshold`, and
  `slowdown_for_curled_perimeters`; set `enable_overhang_speed` to `false` for the
  previous single-bridge-speed behaviour.
- **Advanced retraction modes** — the G-code generator now supports firmware
  retraction (`G10`/`G11`, synced to the firmware with `M207`/`M208` on Marlin or
  `SET_RETRACTION` on Klipper), relative extruder distances (`M83`), a
  configurable minimum-travel-before-retract, a restart-extra prime on recover,
  retract-on-layer-change, and wipe-while-retracting (retracing the just-printed
  path to smear ooze onto printed material, with a configurable
  before-wipe split). Exposed as `use_firmware_retraction`,
  `use_relative_e_distances`, `retract_before_travel_mm`,
  `retract_restart_extra_mm`, `retract_on_layer_change`, `wipe`,
  `wipe_distance_mm`, and `retract_before_wipe_percent`. All default off / to the
  previous behaviour, so existing output is unchanged. The retraction feedrate
  now honours `retract_speed_mm_min` (previously hard-coded). ([#96](https://github.com/max-scopp/slicer-engine/issues/96))
- **Spiral (vase) mode** — the new `spiral_vase` parameter prints a single
  continuous outer wall whose Z ramps smoothly over each layer, producing a
  seamless single-wall vase with no Z-seam. Enabling it forces one perimeter and
  turns off everything that would break the spiral (sparse infill, top surfaces,
  retraction, Z-hop); the solid bottom layers are kept as the base (set
  `bottom_layers` to `0` for an open tube). The layer-height rise is distributed
  along the perimeter length, flow fades in on the first loop and out on the
  last so both ends of the seam disappear, and only the outermost contour of
  each layer is spiralized — multi-island layers fall back to a normal print
  with a warning. Also available on the CLI as `slice --spiral-vase`. Defaults
  to off, so existing output is unchanged.
- **Release notes inside the app** — a new **Settings → What's New** section lists
  every release, newest first, with the version you're running highlighted and
  scrolled into view. The dialog shown after an upgrade now renders that exact
  same list instead of a separate filtered one, so you can always read back past
  releases from the notes you were just shown. On iPadOS, where dialogs are drawn
  by the OS and can't hold that much content, the update prompt takes you to the
  settings section instead. The version row in **Settings → General** links there
  too.
- **iPadOS / iOS target** — the Tauri shell now builds and runs on iPad, with the
  full Rust slicing engine on-device. `pnpm run ios:doctor` checks the toolchain
  (and `ios:setup` installs what it can), `ios:init` generates the Xcode project,
  and `ios:dev` builds, boots an iPad simulator and runs the app with live
  reload — no interactive device picker. The app wiring moved into
  `ui-desktop/src-tauri/src/lib.rs` so desktop and mobile share one entry point,
  and the iOS build drops the CLI, HTTP server and SQLite modules it cannot use
  (333 dependencies instead of 495). Local-network and App Transport Security
  entries are declared up front, so Moonraker printers are reachable from an
  iPad exactly as they are from the desktop app. See
  [ui-desktop/README.md](ui-desktop/README.md).
- **Perimeter routing & ordering options** ([#98](https://github.com/ColdCrabby/slicer/issues/98)) —
  five new wall parameters, each mirroring the PrusaSlicer / OrcaSlicer keys the
  profile importer used to drop:
  - `external_perimeters_first` — print the outer wall **last** (`false`, the new
    default, matching PrusaSlicer/Orca/Cura for the cleanest visible surface) or
    first (`true`). Reorders per-island beads in both wall generators; extrusion
    amounts are unchanged, only print order.
  - `extra_perimeters` — fill a narrow residual core (thinner than
    `extra_perimeters_max_gap × nozzle`, default `3×`) with extra concentric
    perimeter loops instead of leaving a gap for sparse infill. Wide cores stay
    infill's job, so a solid body is never turned into loops. Default off.
  - `thin_walls` — detect **thin features** (model material too narrow for even
    one full perimeter: engraved text, tapering ribs, a card holder's slot fins)
    and print them as a single centered bead. **Classic generator only** — Arachne
    fills them from the medial axis by construction and ignores the option, which
    the settings UI hides accordingly. On by default.
  - `ensure_vertical_shell_thickness` — back sloped/near-vertical surfaces with
    internal solid infill so the side shell keeps a continuous perpendicular
    thickness. A no-op on flat tops and plain vertical walls. Default off.
  - `avoid_crossing_perimeters` — route travel moves around the inside of the
    outer walls (a visibility-graph detour) instead of dragging the nozzle
    straight across a finished surface. Default off.

  All five default to values that preserve existing behaviour except the
  ordering flip (inner-first is now the default), and none change the extrusion
  amounts the slicing-quality baselines measure.

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

- **Your filament's cooling settings are now actually used.** Fan Speed, Bridge
  Fan Speed, First Layer Fan Speed and Fan Off For First Layers were shown in the
  filament editor and written by every material preset, but the G-code generator
  read none of them — it drove the fan purely from the adaptive fan table. The
  most visible consequence: **the part-cooling fan ran during the first layer for
  every material**, quietly costing bed adhesion on every print. It no longer
  does.

  Fan Speed is now the material's cooling **ceiling** — the adaptive curve is
  clamped to it — which is what keeps ABS/ASA/PC from being blasted at full
  airflow while a heated chamber is trying to hold temperature. Bridge Fan Speed
  became a per-segment boost over bridges (matching how overhang cooling already
  worked) rather than a whole-layer setting, and both it and the overhang boost
  are held back on the layers where cooling is switched off, so a single overhang
  can't defeat the first-layer adhesion gate. Material presets were corrected to
  match: the value that used to land in First Layer Fan Speed was really the
  cooling curve's minimum, so PLA would have blown full-speed at the bed.
  ([#8](https://github.com/max-scopp/slicer-engine/issues/8))

- **Isolated infill specks in narrow wedges** — where a cross-section is locally
  thinner than the average wall count (the 3DBenchy bow tip is the canonical
  case), the interior estimate left a sliver that the walls and gap fill already
  fill, and the scanline dropped a single ~1.3 mm dash into it. That speck is
  disconnected, contributes nothing structurally, and costs a full retract →
  travel → un-retract to reach. A connected infill region too small to hold more
  than one dash (2 mm² at a 0.4 mm nozzle) is now skipped.

  This is an **area** rule on whole regions, not a width rule, so a genuinely
  thin cavity that deserves a lattice keeps every line — and it filters the
  generated paths rather than the region, so the scanline phase (seeded from the
  layer's bounding box) is unchanged and the edit is exactly subtractive.

- **Generator-specific wall options are now hidden for the generator that
  ignores them.** `thin_walls` and `wall_distribution_count` only apply to the
  classic wall generator, and `gap_fill_min_length_mm` only to Arachne, but all
  three were shown unconditionally — offering controls that silently did nothing.
  They now carry schema relevance rules, as does `extra_perimeters_max_gap`
  (shown only when `extra_perimeters` is on).

  This also removes a way to silently delete geometry: `thin_walls` used to gate
  Arachne's whole medial pass, which emits the same bead type for *thin features*
  (material too narrow for one perimeter — a card holder's slot fins) and for
  ordinary gap fill *between* perimeter loops. Turning it off removed both: on a
  filament card caddy that wiped ~50 card-slot fins, opened an unfilled void
  along every wall, and let sparse infill leak into the freed band. Arachne now
  always prints thin features — that is what the generator is for — matching
  PrusaSlicer/OrcaSlicer, where the equivalent option is likewise classic-only.

- **Top-surface "squiggles" where solid fill grazes a wall** — a surface whose
  boundary meets the wall band at a shallow angle was filled with a dense
  micro-serpentine of sub-millimetre stubs hugging the wall, interleaved with
  unfilled wedge voids. The wall-band trim leaves a crescent narrower than one
  bead there, and because the fill direction is near-parallel to it every
  scanline span is a stub. On the Filament Card Caddy's hexagon logo the two
  edges lying 15° off the fill direction carried ≈0.22 mm-wide slivers filled
  with a repeating 0.8 mm-line / 0.6 mm-connector zig-zag whose material was
  **93 % already covered** by the flanking wall bead. Solid top/bottom surface
  regions are now width-filtered before filling: anything narrower than one
  extrusion width is dropped (it cannot hold a bead by construction), while
  thicker geometry is preserved at its **exact original shape**, sharp corners
  included. The two affected caddy edges drop from 22.9 → 2.4 mm and
  24.1 → 2.8 mm of sub-1 mm top surface with total material coverage unchanged
  (≤ 0.004 % on Benchy, Voron cube and caddy alike). This was **not** an
  Arachne-only defect — `classic` produced identical stubs, because the cause
  is in the surface fill rather than the wall generator.

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

- **Viewport-cube ortho snap popped back to perspective on pan/zoom** —
  clicking a cube face to inspect a model in a flat, dimension-true view (e.g.
  a selected face of a slice) lost that view the instant you panned or
  zoomed, which is exactly when you want to hold still: dragging around and
  zooming in to evaluate detail. Only a genuine **rotate** now breaks the
  snap free (past the existing sticky threshold); panning and zooming any
  distance keep the projection flattened, letting you inspect a snapped view
  up close without it ever popping back to perspective.

## [0.1.0] - 2026-08-23

### Added

- Initial slicer engine: STL/OBJ/3MF loading, mesh slicing, Arachne
  variable-width wall generation, top/bottom surface detection, and infill.
- Unified scene engine (single source of truth for object placement) shared by
  the CLI, WebSocket server, and WASM UI.
- Angular UI with a Three.js viewport and G-code preview, plus a Tauri desktop
  shell.
- Command-line interface with `slice`, `info`, and schema-generation commands.
