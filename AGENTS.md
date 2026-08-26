# Slicer Engine - AI Agent Guidance

A high-performance 3D model slicer engine written in Rust, powered by [Clipper2](https://github.com/AngusJohnson/Clipper2) for polygon clipping operations.

## Quick Commands

```bash
# Build and run
cargo build
cargo run

# Test and lint
cargo test
cargo fmt && cargo clippy --all-targets --all-features -- -D warnings

# Cross-platform builds
cargo build --target x86_64-pc-windows-msvc   # Windows
cargo build --target x86_64-apple-darwin      # macOS Intel
cargo build --target aarch64-apple-darwin     # macOS ARM
wasm-pack build --target web                   # WebAssembly
```

Or use **Makefile targets** (Linux/macOS):

```bash
make build-release build-windows build-macos build-wasm test lint fmt
```

## Architecture & Design

### Core Components

| Component                      | Location                                     | Purpose                                                                                       |
| ------------------------------ | -------------------------------------------- | --------------------------------------------------------------------------------------------- |
| **SliceLayer / ExtrusionRole** | [src/core/types.rs](src/core/types.rs)       | Core data structures for a single layer                                                       |
| **Mesh Slicer**                | [src/core/slicer.rs](src/core/slicer.rs)     | Triangle→layer contour extraction (`slice_mesh`)                                              |
| **Surface Generation**         | [src/core/surfaces.rs](src/core/surfaces.rs) | Top/bottom solid surface detection and infill                                                 |
| **Wall Restrictions**          | [src/core/walls.rs](src/core/walls.rs)       | Single-wall first/top-layer constraints                                                       |
| **Infill Boundary**            | [src/core/infill.rs](src/core/infill.rs)     | Interior region calculation and sparse infill                                                 |
| **Pipeline**                   | [src/core/pipeline.rs](src/core/pipeline.rs) | `process_mesh` — orchestrates the full slicing pipeline                                       |
| **Scene Engine**               | [src/scene/](src/scene/)                     | Single source of truth for object placement (CLI / WS / WASM all consume `SceneState::apply`) |
| **Clipper2 Integration**       | [src/core/](src/core/)                       | Geometric polygon clipping operations throughout                                              |
| **Library Interface**          | [src/lib.rs](src/lib.rs)                     | Public API exposing core functionality                                                        |
| **CLI Layer**                  | [src/cli/](src/cli/)                         | User-friendly command-line interface bridging library API to commands                         |
| **Build Configuration**        | [build.rs](build.rs)                         | Platform detection and environment setup                                                      |

### Module Organization

```
src/
├── cli/                    # CLI layer (user-friendly commands)
│   ├── mod.rs             # CLI entry point, command dispatcher
│   ├── commands/          # Command implementations
│   │   ├── slice.rs       # Slice operation command
│   │   └── info.rs        # Information command
│   ├── io/                # File I/O layer
│   │   ├── validation.rs  # Path/file validation
│   │   ├── reader.rs      # File reader implementations
│   │   └── writer.rs      # File writer implementations
│   ├── output.rs          # Output formatting (JSON, GCode)
│   ├── error.rs           # CLI error types
│   └── adapters.rs        # Library API adapters
├── core/                  # Core slicing operations (split by concern)
│   ├── mod.rs             # Re-exports public API + integration tests
│   ├── types.rs           # SliceLayer, ExtrusionRole
│   ├── slicer.rs          # slice_mesh, segment chaining
│   ├── surfaces.rs        # generate_top_bottom_surfaces*, rectilinear infill fill
│   ├── walls.rs           # apply_single_wall_restrictions (per-island), compute_per_island_strip_masks
│   ├── infill.rs          # calculate_interior_region, add_infill_to_layers
│   └── pipeline.rs        # process_mesh (full pipeline orchestrator)
├── scene/                 # Unified scene engine (issue #51 — SSOT for object placement)
│   ├── mod.rs             # Re-exports public API
│   ├── transform.rs       # Transform { translation, rotation: Quat, scale }; apply_transform; Euler-XYZ deg helpers
│   ├── bed.rs             # BedConfig (width/depth/height/origin offsets); From<&MachineConfig>
│   ├── loader.rs          # MeshFormat enum + load_bytes / load_path
│   ├── state.rs           # SceneState, SceneObject, ObjectId
│   ├── ops.rs             # SceneOp (Add/Remove/Translate/Rotate/Scale/SetTransform/Center/Drop/AlignFace) + apply()
│   └── wasm.rs            # SceneHandle wasm-bindgen exports (cfg(target_arch="wasm32"))
├── lib.rs                 # Public library root
└── main.rs                # Application entry point (uses CLI)
```

- **lib.rs**: Public library root - re-exports core module and CLI
- **core/**: Core data structures and operations split by concern; `mod.rs` re-exports the public API so all external callers (`crate::core::*`) remain unchanged
- **cli/**: CLI layer providing user-friendly commands and file I/O
- **main.rs**: Application entry point that delegates to CLI layer

### Cross-Platform Strategy

The build script ([build.rs](build.rs)) detects target platform and sets environment variables:

- **Windows**: x86_64-pc-windows-msvc
- **macOS**: x86_64-apple-darwin (Intel), aarch64-apple-darwin (Silicon)
- **WebAssembly**: wasm32-unknown-unknown with wasm-pack

## Development Conventions

### CLI Layer Architecture

The CLI layer uses the **adapter pattern** to bridge the library API to user-friendly commands:

- **Separation of Concerns**: CLI commands in `src/cli/` don't modify core library code
- **Error Handling**: Custom `CliError` type provides user-friendly error messages
- **Output Formatting**: Pluggable formatters support JSON and human-readable outputs
- **File I/O**: Dedicated `io/` submodule handles all file operations with validation
- **Backward Compatibility**: Library API remains unchanged; CLI is purely additive

Example CLI Usage:

```bash
# Slice a model with 0.2mm layer height
slicer-engine slice --input model.stl --layer-height 0.2 --output result.gcode

# Display build information
slicer-engine info --verbose

# Get help on any command
slicer-engine slice --help
```

### Code Style

- Follow [Rust Edition 2021 conventions](https://doc.rust-lang.org/edition-guide/rust-2021/index.html)
- Use `cargo fmt` for formatting (enforced by CI)
- Run `cargo clippy -- -D warnings` before committing
- Write inline tests with `#[cfg(test)]` in the same module

### Performance Priorities

- **Development builds prioritized locally**: Use `cargo build` (debug/opt-level 1) for fast iteration (~5-10s)
  - Release builds with LTO/opt-level 3/codegen-units 1 are reserved for CI/distribution only
  - Profile edge cases with `cargo flamegraph` if performance regressions suspected
- **CI builds**: GitHub Actions builds with `--release` to test final optimized product
- Minimize allocations in hot paths (especially in slicing operations)
- Consider compile-time vs runtime tradeoffs for polygon operations

### Documentation

- Use doc comments (`///`) for public types and functions
- Include usage examples in doc comments for core APIs
- Update [README.md](README.md) for user-facing changes

#### Module READMEs — house style

Long-form module docs (`src/<module>/README.md`) follow the
[Diátaxis](https://diataxis.fr/) **Explanation** quadrant — they discuss what
something is and _why_ it is that way, not how to call every function (that's
what `///` doc comments are for). Reference [src/scene/README.md](src/scene/README.md)
as the canonical example. Conventions:

- **Open with a one-sentence answer to "what does this module exist for?"**
  followed by the single rule or invariant the rest of the doc defends.
- **Lead with motivation, then contract, then anatomy.** Why → rules → shapes
  → catalog → role in the wider system → lifecycle → non-goals.
- **Sprinkle small Mermaid diagrams** where a picture saves a paragraph. Prefer
  several focused diagrams (one `flowchart`, one `classDiagram`, one
  `sequenceDiagram`) over one monster graph. Keep node labels short.
- **Compact tables for catalogs** (ops, variants, flags) — three or four columns
  max; one-line cells.
- **State the non-goals explicitly.** A "what this module deliberately does
  _not_ do" section prevents future drift back into anti-patterns.
- **Plain language over jargon.** Assume a contributor who knows Rust but is
  new to _this_ subsystem. Define a term the first time it appears.
- **End with a "See also" pointing at the source files**, the relevant AGENTS.md
  section, and the originating issue/PR.

### Testing

- Write tests inline with `#[cfg(test)]` modules
- Use `cargo test` to verify release build compatibility
- Test all three platforms: native, WASM, and cross-compilation

## Project Dependencies

| Crate        | Version | Usage                                  |
| ------------ | ------- | -------------------------------------- |
| **clipper2** | 0.5     | Polygon clipping, geometric operations |

_Note: Keep clipper2 dependency current for bug fixes and performance improvements._

## Common Tasks

### Adding a CLI Command

1. Create command module in `src/cli/commands/your_command.rs`
2. Define command struct with `#[derive(Parser)]` from clap
3. Implement command logic using library API adapters
4. Register in `src/cli/commands/mod.rs` and main dispatcher
5. Add error handling with `CliError` conversions
6. Test with `cargo run -- your-command --help`

See [architecture-cli-layer-1.md](plan/architecture-cli-layer-1.md) for detailed implementation phases.

### Adding a New Data Structure

1. Create in appropriate module (usually core.rs)
2. Implement `Debug` and `Clone` traits for inspection and flexibility
3. Add inline tests within `#[cfg(test)]` block
4. Document with `///` doc comments including examples
5. Re-export from lib.rs if part of public API

### Implementing Geometric Operations

1. Leverage Clipper2 API for polygon clipping (avoid reimplementing)
2. Define clear input/output types using SliceLayer or similar structures
3. Write tests covering edge cases (empty paths, degenerate polygons, etc.)
4. Profile performance on large datasets (>10k paths)
5. Document assumptions about coordinate precision

### Cross-Platform Testing

1. Use conditional compilation (`#[cfg(...)]`) for platform-specific code
2. Test locally: `cargo test`
3. Test WASM builds with `wasm-pack test --headless --firefox`
4. Verify CI passes all platform targets before merging

### Adding a Database Migration

1. Use the `sea-orm-cli` tool to scaffold the migration file:
   ```bash
   sea-orm-cli migrate generate "your_migration_name" -d src/db
   ```
2. Implement the schema changes in the generated file's `up` and `down` methods.
3. Register the new module in `src/db/migrations/mod.rs`.
4. Add the migration to the `migrations()` vector in `src/db/migrator.rs`.

## CI/CD Pipeline

GitHub Actions ([.github/workflows/build.yml](.github/workflows/build.yml)) automatically:

- Runs on push and pull requests
- Builds all three platform targets
- Runs linting (clippy) and formatting checks (fmt)
- Executes test suite

**Do not bypass CI checks.** All builds must pass before merge.

## Versioning & Releases — SSOT Contract

**A git tag is the single source of truth for a release.** Never hardcode a
user-facing version number.

- **Version is derived at build time** by [build.rs](build.rs) via `git describe`:
  a clean checkout on an exact `vX.Y.Z` tag reports `X.Y.Z`; everything else
  (ahead of a tag, dirty tree, no tags) reports `development`.
- **[src/version.rs](src/version.rs) is the one place** every target reads version
  and changelog from (`crate::version::VERSION`, `CHANGELOG`, `app_info()`,
  `changelog_entries()`). The CLI (`--version`, `info`, `changelog`), WS
  `Connected`, the WASM exports (`appVersion`/`appInfo`/`changelogMarkdown`/
  `changelogEntries`), and the desktop app all funnel through it. Do not add a
  parallel version constant (especially not in the UI).
- **[CHANGELOG.md](CHANGELOG.md) is embedded** via `include_str!` and republished
  verbatim as GitHub Release notes. Keep an `## [Unreleased]` section at the top.
- **The UI "What's New" dialog** ([ui/src/app/services/app-version.ts](ui/src/app/services/app-version.ts))
  compares the running release against `localStorage` and shows skipped notes
  once per upgrade. Development builds are never nagged.
- **Releasing is tag-driven**: [.github/workflows/release.yml](.github/workflows/release.yml)
  fires on `v*` tags, extracts the changelog section, creates the GitHub Release,
  and attaches CLI binaries + desktop bundles. See [RELEASING.md](RELEASING.md).
  Locally, the [`release` skill](.github/skills/release/SKILL.md) curates the
  changelog (biggest features first, first-time contributors spotlighted) and
  drives tag + push.
- **Cargo.toml `version`** is the *next* target version only — not what users see.

## Known Constraints & Pitfalls

- **Clipper2 Coordinate System**: Uses integer-based `Centi` (centimeter precision). Be aware when converting from floating-point models.
- **CLI Framework**: Uses clap v4 for argument parsing. Keep derive macros in sync with command requirements.
- **WASM Memory**: Be mindful of WebAssembly memory limits when processing large 3D models.
- **File I/O in WASM**: CLI file operations require JavaScript bindings; not all features available in WASM target.
- **LTO Compilation**: Release builds are slower due to LTO. Use debug builds during iterative development.
- **Cross-compilation**: Requires appropriate target toolchains installed. CI verifies these work.
- **`apply_single_wall_restrictions` is per-island**: Inner walls are stripped only from the specific island whose top-surface run ends on that layer; other islands on the same layer are untouched. The `pre_strip_infill_regions` snapshot is still taken before this step to guard against future regressions — keep that order.

## Printer connectivity & G-code cache

[src/printer/](src/printer/) is the **native-only** (`cfg(not(target_arch = "wasm32"))`)
outbound transport to real printers. Today it implements **Moonraker/Klipper**
(`check_status`, `send_gcode`) over `reqwest`.

- **Prefer slicer → printer, not browser → printer.** Probes and uploads run
  server-side (WS `CheckPrinter` / `SendToPrinter` → `PrinterStatus` /
  `PrinterSendResult`) precisely so they are **not subject to CORS** — Moonraker
  ships no permissive `Access-Control-*` headers, so a direct browser `fetch`
  fails for most users. The wasm/`web` build has no native transport and falls
  back to a browser `fetch`; the UI ([printer-connection.ts](ui/src/app/services/printer-connection.ts))
  distinguishes *unreachable* from *reachable-but-CORS-blocked* (via a `no-cors`
  follow-up probe) and surfaces a distinct `cors` status instead of a misleading
  green/offline dot.
- **`PrinterConnection` is the data model** ([src/profiles/printer.rs](src/profiles/printer.rs)):
  `kind`, `host` (may embed scheme/`:port`), `port`, `api_key`, plus the legacy
  UI-owned `connected` flag (no longer trusted for the status dot). Never put
  `reqwest` in `profiles` — it compiles on wasm; keep the transport in the
  native-gated `printer` module.
- **Home-page status dot** reflects the *live* probe, not `connected`: neutral
  (local/unknown), green (online), amber (checking/cors/error/unsupported), red
  (offline).

## G-code result cache — skip re-slicing identical scenes

`handle_slice` ([src/server/ws_session.rs](src/server/ws_session.rs)) hashes the
resolved `SlicingParams` + the ordered scene DTOs (file id + transform) +
`crate::version::VERSION` into an FNV-1a key. A `gcode_cache` table
(migration `m20250201_000002`) maps that key → the previously-generated
`.gcode`. On a hit the pipeline is skipped entirely: the cached file is copied
under the new workplate UUID and `SliceComplete` is emitted immediately. On a
miss the fresh slice is stored. Notes:

- **Object order is preserved in the key** (it affects the merged mesh, hence
  the output). Do not sort.
- **The engine version is part of the key**, so output changes across releases
  bust the cache automatically.
- Cache is best-effort: a dangling row (file cleaned up) is evicted lazily on
  lookup and the scene re-sliced.

## Profile library — persisted next to the engine

User-owned profile *instances* (printers, filaments, process profiles, and the
flat label vocabulary) must live **where the engine runs**, not only in the
browser's `localStorage` — otherwise a cloud user who clears their browser
silently loses every printer/filament even though the slicer is safe on a
server. [src/profiles/store.rs](src/profiles/store.rs) is the engine-side store.

- **TOML at rest, JSON on the wire.** The library is `profiles.toml` beside
  `slicer.toml` in [`config_dir()`](src/config/io.rs); the UI↔engine transport
  is JSON. The profile structs are JSON-native (`#[serde(flatten)]` meta + a
  dynamic `serde_json::Value` `params` bag) which the `toml` serializer cannot
  encode directly, so `ProfileStore` bridges through `serde_json::Value` and
  drops nulls. **Do not try to `toml::to_string` a profile struct directly.**
- **SQLite is only history + `gcode_cache`.** Profiles never touch the DB.
- **Whole-category, last-writer-wins sync.** The unit is a category
  (`ProfileKind::{Printers,Filaments,Processes,Labels}`); the UI sends the full
  array on any add/edit/delete, mirroring the old whole-blob `localStorage`
  write. Single-tenant — one library per engine instance, no auth/identity.
- **Three transports, one store.** Server: `GET /api/profiles` +
  `PUT /api/profiles/:kind` ([server/handlers.rs](src/server/handlers.rs)).
  Native: Tauri `profiles_load` / `profiles_save_category`
  ([ui-desktop/src-tauri/src/commands.rs](ui-desktop/src-tauri/src/commands.rs)).
  Both call the same `ProfileStore`.
- **Change fan-out over WS (cloud only).** A successful `PUT /api/profiles/:kind`
  broadcasts `ServerMessage::ProfilesChanged { kind }` to every open WebSocket
  session (via a `tokio::broadcast` channel on `AppState`), so a second tab
  refetches instead of showing stale profiles.
  [`ProfileSync`](ui/src/app/services/profiles/profile-sync.ts) maps the token
  to its store and calls `reload()` (a cache-bypassing `reloadLibrary()` fetch).
  Inert in web/native — `SlicerConnection.messages$` is `EMPTY` there and those
  runtimes have no second client. GET/PUT stay REST; only the *nudge* is WS.
- **`loadLibrary()` is memoised.** The four stores hydrate in their
  constructors, so `ProfilePersistence.loadLibrary()` shares one in-flight
  request instead of fetching the whole library once per category. Invalidated
  on `saveCategory`; force-refreshed via `reloadLibrary()`.
- **UI: [`ProfilePersistence`](ui/src/app/services/profiles/profile-persistence.ts)**
  has three adapters (browser / remote-REST / native-invoke) picked by
  [`resolveRuntimeMode()`](ui/src/app/runtime/domain/runtime-mode.util.ts) —
  **not** `environment.runtimeMode` alone, because the desktop build ships the
  `cloud` environment and only becomes `native` by detecting Tauri at runtime.
  `localStorage` stays a fast cache in every mode; engine-backed runtimes also
  hydrate from and write through to the store. On first run against an empty
  engine store the local library is pushed up (migration), never clobbered.
- **UI "print profiles" == engine "processes".** The store key is
  `profiles.printProfiles` but the wire/category token is `processes`.
- **`Label` is snake-case aligned** (`{id,name,color,tone}`) across
  [store.rs](src/profiles/store.rs) and
  [label.model.ts](ui/src/app/models/label.model.ts). The UI profile models are
  the engine's generated types (snake_case, no mapping layer), so store items
  serialize to the engine byte-compatibly.
- **The settings-sidebar notice** reflects this: native = "saved on this
  device", cloud = "saved on the slicer" (safe if the browser is wiped), web =
  "kept in this browser only" (losable).

## Scene Engine — SSOT Contract

[src/scene/](src/scene/) is the **single source of truth** for object placement, orientation, and transforms. Issue #51 introduced it; CLI, WS server, and the Angular UI (via WASM) all consume the same `SceneState::apply()` code path. Every CLI flag and every UI gesture must translate to a `SceneOp`.

- **Math**: `glam::{Vec3, Quat, Mat4}`. Quaternions internally; **Euler-XYZ degrees only at protocol/CLI boundaries** (see `Transform::from_euler_xyz_deg` / `to_euler_xyz_deg`).
- **Ops** (`SceneOp`): `Add`, `Remove`, `Translate`, `SetTransform`, `Rotate`, `Scale`, `CenterOnBed`, `DropToFloor`, `AlignFaceToFloor`. Each `apply` returns an `OpReceipt { inverse }` — sets up undo without implementing it.
- **AlignFaceToFloor**: picks face by index, computes `Quat::from_rotation_arc(world_normal, -Z)`, then drops to floor.
- **Bake at the slicer boundary only**: `apply_transform(&Mesh, &Transform) -> Mesh` is called once before the slicing pipeline runs. Never bake mid-pipeline.
- **Object IDs**: `ObjectId(u64)` is monotonically allocated and **never reused**. UUIDs are reserved for the WS protocol's upload tokens, not for scene objects.
- **Server scenes are ephemeral per WS connection** (no DB persistence). UI uploads bytes via the file-upload endpoint, then dispatches `Scene { ops: [Add { file_id }, …] }`.
- **WASM** (`src/scene/wasm.rs`, `cfg(target_arch="wasm32")`): exposes `SceneHandle` with `addMesh`, `applyOp`, `getRenderBuffer`, `getMatrix`, `snapshot`. JS bindings build via `make build-wasm` → `ui/src/generated/scene-wasm/`.
- **Wasm vs native deps**: `clipper2`, `zip`, `uuid`, `rayon`, `tobj`, `actix-*`, `tokio`, `rusqlite` are gated `cfg(not(target_arch="wasm32"))`. The wasm build only ships `mesh`, `scene`, `logging`, plus wasm-only `wasm-bindgen`/`js-sys`/`serde-wasm-bindgen`. Module-level `#[cfg]`s in `lib.rs` enforce this.
- **Deprecated CLI flags**: `--center` / `--drop-to-floor` are kept as aliases that log a deprecation warning and dispatch the equivalent `SceneOp`. Do not add new flags that bypass the scene engine.
- **Don't add a parallel mesh placement path**. The temptation to "just translate this mesh real quick" in `mesh::transforms` is exactly what issue #51 set out to eliminate.

## Slicing Pipeline — Deep Knowledge

This section records hard-won understanding of how the slicing pipeline works and
why specific design decisions were made. Read this before touching anything in
[src/core/](src/core/) or [src/arachne/mod.rs](src/arachne/mod.rs).

**Validate, don't guess.** [tools/gcode-analysis/](tools/gcode-analysis/README.md)
measures sliced G-code directly — wall overlap (`coincident.py`), unfilled
wall-zone gaps (`voids.py`), length-weighted bead widths (`widthdist.py`), and
capsule/gap renders (`render.py`, `zoom.py`). Compare a change against the
`classic` generator (the trusted reference) before claiming a fix.

### Pipeline Execution Order

```
slice_mesh()                         — raw mesh → OuterWall contours per layer
generate_arachne_walls()             — replaces OuterWall contours with bead paths
pre_strip_infill_regions computed    — interior regions snapshotted before wall stripping
apply_single_wall_restrictions()     — strips inner walls from first/last layers if configured
interior_regions computed            — per-layer interior (for surfaces), post-strip
generate_top_bottom_surfaces_with_interior()  — top/bottom solid infill within interior
add_infill_to_layers()               — sparse infill using pre-strip regions minus solid regions
```

Order matters critically. Surfaces are computed **after** Arachne walls so that
`calculate_interior_region` sees the correct bead geometry. Infill is computed
**after** surfaces so it can subtract `solid_regions`.

**`pre_strip_infill_regions` must be computed before `apply_single_wall_restrictions`.**
`apply_single_wall_restrictions` now operates **per island**: an outer-wall path P at
layer i has its associated inner walls stripped only when P's footprint has an exposed
top surface AND P does not appear in layer i+1 (the island ends here). The large body
island on the same layer is unaffected. The `pre_strip_infill_regions` snapshot is
still taken before this step as a defensive measure — the snapshot preserves the correct
`walls_per_island` count for every island in case future changes ever re-introduce a
layer-wide strip.

### Arachne Wall Paths — What They Are and Are Not

Arachne emits **centerline paths**, not filled polygons. Each path is a closed
polygon whose vertices are the _center_ of the extrusion bead, not its edge.

- `OuterWall` paths sit at inward depth `d/2` from the raw mesh contour.
- `InnerWall` paths sit at `3d/2`, `5d/2`, … from the outer contour.
- `path_widths[i]` carries the actual extrusion width for variable-width beads.
- For a mesh with holes (donut, hollow cylinder) the **hole boundary** also gets
  an `OuterWall` tag (`is_outer = true` in Arachne, because it is the outermost
  bead of that contour's shrink sequence). There is no separate "hole wall" tag.

Consequence: you **cannot** tell an outer solid contour from a hole contour by
role alone. Use signed area (`path.signed_area()`): CCW (positive) = solid
island, CW (negative) = hole.

**`GapFill` beads follow a de-noised residual medial axis.** The variable-width
gap-fill that closes the thin residual between the innermost wall and the infill
boundary is walked from the residual's segment-Voronoi skeleton. That raw
skeleton is noisy: a spur grows at every faceted boundary vertex, splitting the
gap spine into a chain per junction whose stubs wander from layer to layer (a
curved hull's residual ring boils into a different set of short beads each
layer). [`prune_short_leaf_chains`](src/walls/arachne/skeleton.rs) removes spurs
shorter than `2·nozzle` so those junctions collapse to degree 2 and the spine
reassembles into a few long continuous beads (on the Benchy hull: ~⅓ the bead
count, ~3× the mean bead length, no void-coverage change). Keep that floor near
`2·d` — the radius-ratio `prune_boundary_spurs` misses uniform-radius facet
spurs, and a much larger floor erodes genuine sub-millimetre features into
voids. Walls are untouched, so this never affects the coincidence-free property.

### Clipper2 Fill Rules — When to Use Which

| Operation                                                                 | Fill rule  | Why                                                                                                                                    |
| ------------------------------------------------------------------------- | ---------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| Surface detection (intersect/difference of layer perimeters)              | `EvenOdd`  | Mesh slicer does not guarantee consistent winding; EvenOdd is winding-independent                                                      |
| Infill interior subtraction (`difference` of infill area − solid regions) | `Positive` | Infill area from `calculate_interior_region` uses consistent Clipper2 winding; Positive is more predictable for non-overlapping inputs |
| Arachne bead union (old approach, now removed)                            | `NonZero`  | Would require all CCW; don't use unless winding is normalised first                                                                    |

**Do not union Arachne bead paths with `EvenOdd`.** Tightly nested concentric
closed paths under EvenOdd produce alternating in/out bands instead of a single
solid region.

### `perimeter_paths_of` — OuterWall Only

`perimeter_paths_of()` intentionally returns only `OuterWall` paths, even though
layers also have `InnerWall` paths.

**Why**: Surface detection compares adjacent layer geometries with Clipper2
`intersect`/`difference` using `EvenOdd`. If `InnerWall` beads are included,
each bead boundary toggles EvenOdd inside/outside. With e.g. 3 inner walls, the
inter-bead gaps register as alternating "exposed" strips → spurious `BottomSurface`
or `TopSurface` paths appear between the wall beads, indistinguishable from real
surfaces. The `OuterWall` paths alone faithfully represent the solid cross-section
of each island.

### `calculate_interior_region` — How the Infill/Surface Boundary Is Computed

Uses `OuterWall` paths directly as the gross outline of each island (winding
preserved — **do not normalise to CCW**). Deflates inward by:

```
total_inward = (walls_per_island - 0.5) × nozzle_diameter - overlap_distance
```

The `−0.5 × d` term accounts for the fact that `OuterWall` centerlines are
already inset `d/2` from the model surface. Without this correction the interior
region is over-shrunk by half a bead width.

`walls_per_island = ceil(total_wall_bead_count / outer_contour_count)` gives the
number of wall shells per island. This works because Arachne places the same
number of beads on every island (parameters are global, not per-island).

**Do not normalise all wall paths to CCW before the inflate.** Hole boundary
beads have CW winding. Flipping them to CCW makes Clipper2 treat holes as solid
material → infill is generated inside the hole (through the void).

### Infill Boundary vs. Surface Region

`add_infill_to_layers` calls `calculate_interior_region(layer, 0.0, nozzle_diameter_mm)`
(overlap = 0) to get the infill area, then subtracts `layer.solid_regions` with
`FillRule::Positive`.

`generate_top_bottom_surfaces_with_interior` clips surface regions to
`interior_regions[i]` (computed ahead of time with
`calculate_interior_region(layer, infill_overlap_percent, nozzle_diameter_mm)`)
before generating solid infill lines.

Both use `calculate_interior_region` — but with different `overlap_percent`
values. Keep them consistent if the signature changes.

### `generate_rectilinear_infill` — Scanline Even-Odd Fill

The scanline fills cells using an even-odd intersection count (pairs of sorted
X crossings per scan line). This is correct for both simple polygons and for
Clipper2-output `Paths` whose hole sub-paths have CW winding, because the CW
hole adds an extra edge crossing that naturally toggles the parity.

No special handling is needed for holes in the input `Paths` — the algorithm is
correct as-is as long as the input `Paths` has proper Clipper2 winding (CCW
solids, CW holes).

### Infill for Shapes with Holes

For a layer that contains a hole (e.g. a hollow box cross-section), the
`calculate_interior_region` output is a Clipper2 `Paths` with:

- One or more CCW sub-paths (solid ring interior)
- One or more CW sub-paths (the hole voids)

The `inflate` call with a negative delta correctly shrinks the solid ring inward
while simultaneously _growing_ the CW hole outward (toward the ring), preserving
the annular region where infill should go. The scanline in
`generate_rectilinear_infill` then correctly generates lines only inside the
annulus because the hole sub-path's edges produce crossing events that close the
infill within the ring.

## Related Documentation

- [README.md](README.md) - User guide and feature overview
- [RELEASING.md](RELEASING.md) - Versioning + changelog + GitHub Release process
- [CHANGELOG.md](CHANGELOG.md) - Embedded, user-facing release notes
- [SETUP_COMPLETE.md](SETUP_COMPLETE.md) - Initial setup record
- [architecture-cli-layer-1.md](plan/architecture-cli-layer-1.md) - CLI layer implementation plan
- [tools/gcode-analysis/](tools/gcode-analysis/README.md) - G-code quality diagnostics (wall overlap, unfilled gaps, bead widths, render/zoom)
- [Clipper2 Documentation](https://github.com/AngusJohnson/Clipper2) - Polygon clipping reference

---

**Last Updated**: 2026-08-23 (versioning + changelog + GitHub Releases)  
**Maintainer Guidance**: Keep this file in sync with project structure changes, new conventions, or significant architectural decisions.
