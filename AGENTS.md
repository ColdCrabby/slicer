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
| **Multi-object plates**        | [src/core/objects.rs](src/core/objects.rs)   | `slice_plate` — per-object identity for exclude-object & sequential printing                  |
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
│   ├── pipeline.rs        # process_mesh (full pipeline orchestrator)
│   └── objects.rs         # slice_plate — multi-object plates, object identity (#22/#112)
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

Docs are split by audience, and a change usually belongs in exactly one place.

| Audience | Lives in | Style |
| --- | --- | --- |
| **Users** — how to use the app | [docs/use/](docs/use/) | Plain language, task-first. Simple by default; advanced detail kept but terse, in `::: details` blocks. |
| **Teams / businesses** — deploying and operating it | [docs/teams/](docs/teams/) | Self-hosting, shared config, automation, data & licensing. Assumes an administrator. |
| **Brand** | [docs/brand.md](docs/brand.md) | Name, mascot, assets, palette, voice. |
| **Contributors** — how it works | Module `README.md`s, [ARCHITECTURE.md](ARCHITECTURE.md) | Explanation, not reference. See house style below. |

- Use doc comments (`///`) for public types and functions
- Include usage examples in doc comments for core APIs
- **A user-visible change is not done until [docs/use/](docs/use/) reflects it.**
  A new setting, a new button, a changed shortcut — all of it.
- Keep [ARCHITECTURE.md](ARCHITECTURE.md) a *map*. Depth belongs in the module
  README it points at.
- Update [README.md](README.md) for headline feature changes only.

**The docs site carries a temporary "early docs" banner** while the structure
and tone settle. It is a `layout-top` slot in
[docs/.vitepress/theme/](docs/.vitepress/theme/); delete `Banner.vue`,
`banner.css`, and the `theme/` directory's registration in `index.ts` to remove
it. `--vp-layout-top-height` in `banner.css` is what reserves its space — drop
that with it, or the nav keeps a gap above it.

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

See [src/cli/README.md](src/cli/README.md) for the command catalog and argument reference.

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
- **The UI renders the changelog from exactly one component**
  ([ui/src/app/components/changelog/changelog-list.ts](ui/src/app/components/changelog/changelog-list.ts)).
  It always lists *every* release and highlights/scrolls to the one you're
  running, so the **What's New** settings section (`/settings/changelog`) and the
  post-upgrade dialog can never drift apart.
  [ui/src/app/services/app-version.ts](ui/src/app/services/app-version.ts)
  compares the running release against `localStorage` and shows that dialog once
  per upgrade; development builds are never nagged and highlight `Unreleased`.
  **Where dialogs are drawn by the OS** (iOS/iPadOS — see `Dialog.usesNativeDialogs()`)
  a `UIAlertController` cannot hold the changelog, so the prompt is a short
  native confirm that navigates to the settings section instead.
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

## Native shell targets — desktop and iPadOS/iOS

[ui-desktop/](ui-desktop/README.md) is one Tauri shell that ships to macOS,
Windows, Linux **and iPadOS/iOS**. Three contracts hold it together.

- **The app entry point is a library, not a `main`.**
  [ui-desktop/src-tauri/src/lib.rs](ui-desktop/src-tauri/src/lib.rs) builds and
  runs the `tauri::Builder`; `main.rs` is a three-line desktop launcher. iOS has
  no Rust `main` — the generated Xcode project links this crate as a
  **`staticlib`** and calls the symbol `#[cfg_attr(mobile, tauri::mobile_entry_point)]`
  exports. **Never move app wiring back into `main.rs`**: it would compile fine
  on desktop and silently never run on mobile. Desktop-only setup (window
  decorations, accent-colour watcher) belongs inside `#[cfg(desktop)]`.
- **`cli`, `server` and `db` are excluded from iOS.** Gated in
  [src/lib.rs](src/lib.rs) with `cfg(all(not(target_arch = "wasm32"), not(target_os = "ios")))`
  and mirrored by the target-specific dependency tables in
  [Cargo.toml](Cargo.toml), which keep `clap`, the `actix-web` stack and
  `sea-orm`/`sqlx` off Apple mobile targets entirely (333 packages vs 495 for
  macOS). A sandboxed app has no command line and must not bind a listener.
  `printer` **stays** — sending G-code from an iPad uses the same native,
  CORS-free transport as desktop. **Change the `cfg`s and the dependency tables
  together**, and verify with
  `cargo metadata --filter-platform aarch64-apple-ios-sim`.
- **Capabilities are split by platform.** `capabilities/default.json` is
  cross-platform; `capabilities/desktop.json` carries `"platforms": ["macOS", "windows", "linux"]`
  and holds every window-chrome grant. Do not add window permissions to the
  default capability.

Supporting details:

- **`Info.ios.plist`** (next to `tauri.conf.json`) is merged into the generated
  `Info.plist`, so it survives re-running `ios init`. It is what makes LAN
  printers work at all: iOS 14+ blocks local networking without
  `NSLocalNetworkUsageDescription`, and ATS blocks Moonraker's plain HTTP
  without `NSAllowsLocalNetworking`.
- **`gen/apple` is generated but committed** (only `gen/schemas` and build
  output are ignored) — it carries signing settings and the merged plist.
- **UI:** mobile is *not* a new runtime mode. `resolveRuntimeMode()` still
  reports `native` on iPad; anything that draws or drives **native chrome** asks
  [`isTauriMobile()` / `isTauriDesktop()`](ui/src/app/runtime/domain/runtime-mode.util.ts)
  instead. **Gate every `@tauri-apps/api` module that Tauri marks
  `#[cfg(desktop)]` — `menu` and most of `window` — behind `isTauriDesktop()`,
  not `isTauriHost()`.** Those commands do not exist on mobile, so the call
  rejects and the feature silently vanishes instead of falling back. That is
  exactly how iPad lost its context menus: `ContextMenuService` asked
  `isTauriHost()` and tried to build an OS menu that iOS has no API for.
  Measured on the simulator, an iPad reports UA `Macintosh…`, platform
  `MacIntel` and **no** `iPad` token — a user-agent sniff alone classifies it as
  a desktop Mac, so the helper keys off `maxTouchPoints` as well.
- **Long-press is the touch equivalent of right-click.**
  [`ContextMenuTrigger`](ui/src/app/services/context-menu/context-menu-trigger.ts)
  synthesises it because iOS never fires `contextmenu` for a long press. Two
  non-obvious requirements: the trailing `click` produced when the finger lifts
  must be swallowed **wherever it lands** (the menu opens at the pointer, so
  restricting the guard to the host lets that click activate a menu item
  instantly), and the menu is offset further on touch so it does not open under
  the fingertip. iOS also needs `-webkit-touch-callout: none` on trigger
  elements (see `styles/base/_reset.scss`) or its own selection callout hijacks
  the gesture.
- **Context menus, dialogs and file export are drawn by the OS on iOS too, not
  just on desktop.** Tauri's `menu` module is desktop-only and UIKit's
  `UIContextMenuInteraction` cannot be presented imperatively, so
  [context_menu.rs](ui-desktop/src-tauri/src/context_menu.rs) builds a
  `UIAlertController` action sheet and
  [native_dialog.rs](ui-desktop/src-tauri/src/native_dialog.rs) supplies native
  alerts plus a `UIActivityViewController` share sheet. **The HTML versions are
  the browser's fallback, not the mobile default**; do not "simplify" mobile
  back onto them. Two are correctness fixes rather than polish: iOS `save()`
  writes a 0-byte file (no Save-As panel exists), and the iOS file picker greys
  out `obj`/`3mf` unless `Info.ios.plist` declares their UTIs. Any popover on
  iPad (action sheet, share sheet) **must** set `sourceView`/`sourceRect` or
  UIKit raises and the app terminates. Full rationale in
  [ui-desktop/README.md](ui-desktop/README.md#which-surfaces-are-native).
- **Dev environment:** [scripts/ios-doctor.sh](scripts/ios-doctor.sh) verifies
  the toolchain (full Xcode — *not* Command Line Tools, which lack the iOS SDK —
  simulator runtimes, Rust `aarch64-apple-ios{,-sim}` targets, CocoaPods) and
  `--fix` installs what it can. `pnpm run ios:dev` auto-selects an iPad
  simulator, since `tauri ios dev` otherwise prompts with mostly iPhones.
- **`sudo xcode-select -s /Applications/Xcode.app` is unavoidable.** Installing
  Xcode does not switch the active developer directory. `DEVELOPER_DIR` fixes
  the helper scripts (`simctl`) without sudo, but **`tauri ios dev` builds with
  a sanitized environment and never forwards it**, so its `xcodebuild` still
  resolves the Command Line Tools. Do not "fix" this with a `DEVELOPER_DIR`
  export in the scripts — it silently does nothing for the actual build.
- **`ui-desktop/package.json` must keep its `"tauri": "tauri"` script.** The
  generated Xcode "Build Rust Code" phase runs `pnpm tauri …` from `gen/apple`;
  without that passthrough the iOS build dies with `Command "tauri" not found`.
- **App icons come from one master via `pnpm run icons`.**
  `ui-desktop/src-tauri/app-icon.png` (1024², opaque, cropped from
  `ui/public/logo_source.png`) feeds every platform. Do not hand-edit the
  generated sets. `tauri ios init` seeds Tauri's *placeholder* logo, so the
  icons must be regenerated after it — and `tauri icon` writes iOS icons as RGBA
  even with `--ios-color`, which App Store Connect rejects (`ITMS-90717`), so
  [scripts/gen-icons.sh](scripts/gen-icons.sh) flattens them back to RGB and
  prunes the UWP/Android output we never ship.
- **Xcode 26 ships no simulator runtime** — `xcodebuild -downloadPlatform iOS`
  fetches ~8 GB separately. A staged *image* (`simctl runtime list`) is not a
  registered *runtime* (`simctl list runtimes`); if the two disagree the image
  is broken. All images of a version share one asset, so deleting a duplicate
  breaks the survivor — purge with `simctl runtime delete all` and re-download.

## Printer connectivity & G-code cache

[src/printer/](src/printer/) is the **native-only** (`cfg(not(target_arch = "wasm32"))`)
outbound transport to real printers. Today it implements **Moonraker/Klipper**
(`check_status`, `detect_printer`, `send_gcode`) over `reqwest`. It is reached by
**both** native runtimes over the OS network line — never the browser:

- **Cloud** `serve` WebSocket: `CheckPrinter` / `DetectPrinter` / `SendToPrinter`.
- **Desktop** Tauri commands: `printer_check` / `printer_detect` / `printer_send`
  ([ui-desktop/src-tauri/src/commands.rs](ui-desktop/src-tauri/src/commands.rs)),
  which call the same `crate::printer` functions in-process. Their result types
  (`PrinterStatusReport`, `PrinterDetection`, `SendOutcome`) are `Serialize`d to
  the **same field shape** as the WS `PrinterStatus` / `PrinterDetected` /
  `PrinterSendResult` payloads (minus the envelope), so the UI reuses one set of
  `fromServer*` mappers for both transports.

- **Prefer slicer → printer, not browser → printer.** Probes and uploads run in
  the native process precisely so they are **not subject to CORS** — Moonraker
  ships no permissive `Access-Control-*` headers, so a direct browser `fetch`
  fails for most users. Only the pure wasm/`web` build has no native transport
  and falls back to a browser `fetch`; the UI
  ([printer-connection.ts](ui/src/app/services/printer-connection.ts)) picks the
  transport at runtime — cloud WS when connected, Tauri commands when
  `isTauriHost()`, browser `fetch` only in `web`. **Do not gate on
  `environment.runtimeMode` alone**: the desktop build ships the `cloud`
  environment and only becomes native by detecting Tauri at runtime, so a
  build-time constant would send the desktop app down the CORS-prone browser
  path (the bug this contract exists to prevent). The web fallback still
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
resolved `SlicingParams` (via `SlicingParams::cache_fingerprint`) + the ordered
scene DTOs (file id + transform) + `crate::version::VERSION` into an FNV-1a key.
A `gcode_cache` table (migration `m20250201_000002`) maps that key → the
previously-generated `.gcode`. On a hit the pipeline is skipped entirely: the
cached file is copied under the new workplate UUID and `SliceComplete` is emitted
immediately. On a miss the fresh slice is stored. The desktop (Tauri) runtime
keeps an in-memory mirror with the same key
([ui-desktop/src-tauri/src/bridge/runtime_bridge.rs](ui-desktop/src-tauri/src/bridge/runtime_bridge.rs)).
Notes:

- **Object order is preserved in the key** (it affects the merged mesh, hence
  the output). Do not sort.
- **The engine version is part of the key**, so output changes across releases
  bust the cache automatically.
- **The embedded thumbnail PNG is excluded from the key.**
  `SlicingParams::cache_fingerprint` drops `thumbnail_png_base64` (the
  camera-derived preview captured fresh from the viewer on every slice) so its
  volatile bytes never bust the cache — the issue #106 requirement that camera
  movement leave the cache-hit rate unaffected. The thumbnail *settings*
  (`thumbnail_view`/`theme`/`size`/…) stay in the key, so a cached file's
  embedded preview always matches the request that reused it.
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
  encode directly, so the conversion goes through `serde_json::Value` with nulls
  dropped. **Do not try to `toml::to_string` a profile struct directly** — call
  [`toml_bridge::render_library_toml`](src/profiles/toml_bridge.rs), the one
  renderer behind both `ProfileStore::save` and the exporter.
- **The library *shape* is target-independent.** `ProfileLibrary`, `Label` and
  `ProfileKind` live in [library.rs](src/profiles/library.rs) so wasm (which has
  no filesystem, hence no `store`) can still use them. Only `ProfileStore` is
  `cfg(not(target_arch = "wasm32"))`.
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

### Export — written once, never revisited

[src/profiles/export.rs](src/profiles/export.rs) renders the library for
download in two shapes: a **bundle** (`slicer-profiles.zip`, one TOML per
profile plus `labels.toml`, `manifest.toml`, `README.md`) and a **single**
`profiles.toml` identical to what the store writes. Both come from the same
`ProfileLibrary` serialization.

- **Never name a field — or a category.** The exporter walks the *serialized
  value* and treats every top-level array as a category. A new profile setting,
  or a whole new category on `ProfileLibrary`, is exported with **zero** changes
  here. That is the point of the module; do not add per-feature branches to it.
  The only category-specific rule is one line (`splits_per_item`): labels are a
  flat vocabulary and stay in one file, everything else is one file per item.
- **Every file is an array of tables** (`[[printers]]`), and per-item files are
  **ordinal-prefixed** (`printers/01-voron-24.toml`). Concatenating a bundle in
  name order therefore reconstructs a valid `profiles.toml` *with the original
  order intact* — the contract a future importer relies on, pinned by
  `concatenating_a_bundle_reconstructs_the_library`.
- **Deterministic:** fixed zip timestamps, so the same library exports
  byte-identically and the artifact can be diffed or version-controlled.
- **Credentials are stripped.** An export is built to be *handed over* (git
  repo, AirDrop, mail), so `redact_secrets` removes any field named in
  `SECRET_FIELDS` (`api_key`, `token`, …) anywhere in the tree, in **both**
  shapes. Matched by name at the value level, so a credential added to any
  profile later is covered for free. The bundle README and the settings copy
  both say so; the user re-enters keys after restoring.
- **The export is faithful to the library *as the engine understands it*** —
  what `ProfileStore::load` produced and what the next save would write — not a
  verbatim copy of the bytes on disk. A *typed* field written by a different
  build is dropped by serde at load, before the exporter sees it (exactly as it
  already is for `GET /api/profiles`). Free-form `params` entries, where new
  slicing settings actually land, always survive. Don't overstate this in
  user-facing copy.
- **Three transports, one renderer:** `GET /api/profiles/export?format=`
  (cloud), Tauri `profiles_export` (native) — both export what is *persisted*,
  i.e. what the CLI on that machine would read — and wasm
  `exportProfileLibrary(library, format)` for the web runtime, where the browser
  is the engine and the UI's own library is the only truth. The wasm binding
  exists only in the `web-slicer` build, so
  [`BrowserProfilePersistence`](ui/src/app/services/profiles/profile-persistence.ts)
  looks it up dynamically rather than importing a symbol the cloud/native
  bindings do not declare.
- **UI:** [`ProfileExport`](ui/src/app/services/profiles/profile-export.ts) picks
  the transport and hands the bytes to
  [`FileExport`](ui/src/app/services/file-export.ts) — the one place that knows
  the three "save a file" idioms (iOS share sheet, desktop Save-As, browser
  anchor). G-code downloads go through it too; **do not re-implement a download
  in a feature**.

## Scene Engine — SSOT Contract

[src/scene/](src/scene/) is the **single source of truth** for object placement, orientation, and transforms. Issue #51 introduced it; CLI, WS server, and the Angular UI (via WASM) all consume the same `SceneState::apply()` code path. Every CLI flag and every UI gesture must translate to a `SceneOp`.

- **Math**: `glam::{Vec3, Quat, Mat4}`. Quaternions internally; **Euler-XYZ degrees only at protocol/CLI boundaries** (see `Transform::from_euler_xyz_deg` / `to_euler_xyz_deg`).
- **Ops** (`SceneOp`): `Add`, `Remove`, `RemoveMany`, `Duplicate`, `Translate`, `SetTransform`, `Rotate`, `Scale`, `CenterOnBed`, `DropToFloor`, `PlaceFaceOnFloor`, `AutoOrient`, `ArrangeOnBed`, `BatchSetTransform`. Each `apply` returns an `OpReceipt { inverse }` — sets up undo without implementing it.
- **AlignFaceToFloor**: picks face by index, computes `Quat::from_rotation_arc(world_normal, -Z)`, then drops to floor.
- **Bake at the slicer boundary only**: `apply_transform(&Mesh, &Transform) -> Mesh` is called once before the slicing pipeline runs. Never bake mid-pipeline.
- **Object IDs**: `ObjectId(u64)` is monotonically allocated and **never reused**. UUIDs are reserved for the WS protocol's upload tokens, not for scene objects.
- **`SceneObject::source_id` is the object→bytes link.** Every object records the opaque handle it was loaded from (the WS upload UUID, a CLI path), set at `Add` time and inherited by `Duplicate`. **Never pair the object list against the upload list positionally** — the two are maintained independently, so index-pairing silently slices the wrong mesh the moment their order or length diverges (it collapsed a two-model plate into two copies of the first model). `Duplicate` shares the original's `Arc<Mesh>` *and* its `source_id`, so N instances of one model cost one upload.
- **`SceneObject::source_part` says *which* object inside that file.** A 3MF is a scene, not a model: `SceneOp::Add` expands a multi-part file into **one scene object per build item** (named from the 3MF's `name` attribute), all sharing one `source_id`. The file id alone is therefore ambiguous — slicing must carry `part_index` too (`SceneObjectSliceDto`), or the server re-loads the whole file for every part and prints each one N times. `load_bytes_multi` / `load_path_multi` are the split loaders; `load_bytes` / `load_path` still merge and are what the slicer sees after a part is picked. The G-code cache key includes `part_index` for the same reason.
- **A multi-part `Add` inverts to `RemoveMany`, not `Remove`.** One `Add` can create many objects, so undo has to take all of them back in one step. `SceneHandle::addMesh` returns an **array** of ids to match.
- **Placement is validated in the engine, not per front-end**: `SceneState::placement_report()` returns `out_of_bounds` (via `BedConfig::contains_aabb`, shape-aware) and `collides` (XY-footprint overlap; touching edges do not count, so `ArrangeOnBed` output is clean) for every object. The WASM snapshot carries both flags per object. Its epsilon is `1e-3` mm, not `1e-9` — STL coordinates are `f32`, so a model resting on the bed lands a few `1e-6` mm below zero and a tighter tolerance reports it out of bounds.
- **Server scenes are ephemeral per WS connection** (no DB persistence). UI uploads bytes via the file-upload endpoint, then dispatches `Scene { ops: [Add { file_id }, …] }`. `POST /api/upload` takes an optional `ruuid` field (sent **before** the file field, since multipart streams in order) that attaches the upload to an existing workplate — that is how one plate accumulates several files so `GET /api/request/:ruuid` can restore all of them.
- **WASM** (`src/scene/wasm.rs`, `cfg(target_arch="wasm32")`): exposes `SceneHandle` with `addMesh`, `applyOp`, `getRenderBuffer`, `getMatrix`, `snapshot`. JS bindings build via `make build-wasm` → `ui/src/generated/scene-wasm/`.
- **Wasm vs native deps**: `clipper2`, `zip`, `uuid`, `rayon`, `tobj`, `actix-*`, `tokio`, `rusqlite` are gated `cfg(not(target_arch="wasm32"))`. The wasm build only ships `mesh`, `scene`, `logging`, plus wasm-only `wasm-bindgen`/`js-sys`/`serde-wasm-bindgen`. Module-level `#[cfg]`s in `lib.rs` enforce this.
- **Deprecated CLI flags**: `--center` / `--drop-to-floor` are kept as aliases that log a deprecation warning and dispatch the equivalent `SceneOp`. Do not add new flags that bypass the scene engine.
- **Don't add a parallel mesh placement path**. The temptation to "just translate this mesh real quick" in `mesh::transforms` is exactly what issue #51 set out to eliminate.

### Multi-object workplates — UI contract

A workplate is a **build plate, not a file**. It starts from one model and must
accept more, so the UI keeps a strict split of responsibilities:

- **[`WorkplateObjects`](ui/src/app/services/workplate-objects/workplate-objects.ts) is the only way an object gets onto a plate.** It uploads (cloud), calls `addMesh` with the resulting `source_id`, places the result using the shared [`Arrange`](ui/src/app/services/arrange/arrange.ts) settings, and nudges the new object clear of the ones already there. Every entry point — the toolbar's add button, drag-and-drop, restoring a saved plate — goes through it, so they cannot drift apart. Adding **never** clears existing objects; only an explicit clear does.
- **Placing objects is one command, not two.** "Auto-orient" and "arrange all" used to be rival buttons that undid each other's work. [`Arrange`](ui/src/app/services/arrange/arrange.ts) owns the single `ArrangeOnBed` dispatch plus the settings it needs (gap, auto-orient, and the printer's preferred angle). Its UI follows the object-tools idiom exactly: a uniform toolbar button **in the same group as move / rotate / scale**, which reveals a contextual card — [`PlacementPanel`](ui/src/app/components/placement-panel/placement-panel.ts), a sibling of [`TransformPanel`](ui/src/app/components/transform-panel/transform-panel.ts). **Do not give it a split caret** — that made one button in the group behave unlike its neighbours. **Add-time placement reads the same settings**: dropping a file in and pressing the button must not disagree about orientation or spacing. Do not re-introduce a bare `AutoOrient`-everything action beside it.
- **Contextual tool cards hang off the tools that open them.** Both cards render inside [3d-view-toolbar.html](ui/src/app/components/3d-view-toolbar/3d-view-toolbar.html), in a `.tool-panels` column **absolutely positioned** under the `.tool-cluster` and centred on it, so they follow the buttons instead of sitting in a screen corner the user has to connect them to. Absolute positioning is what makes this safe: the toolbar's own `contentRect` height is unchanged, so the shell's `--main-scene-inset` (and with it the viewport-cube and slice rail) never shifts as cards appear — that invariant is why the transform card originally lived in the shell. The column stacks, so transform + placement can be open at once; the container is `pointer-events: none` so gaps stay click-through to the scene. The toolbar's own pill rules must keep their `:host ` prefix — `nexus-card` styles itself with `:host(.small){border-radius:var(--radius-md)}`, which a bare class selector ties on specificity and loses to on order, squaring off the pill.
- **[`TransformPanel`](ui/src/app/components/transform-panel/transform-panel.ts) edits the whole selection, not one object.** It used to render nothing unless *exactly one* object was selected, which made a multi-object plate untransformable. Position edits apply as a **delta** off the selection's combined AABB centre (so a spread-out arrangement keeps its layout rather than collapsing onto one coordinate); rotation and scale are set **per object** about each one's own centre, and `setSize` measures each object's own AABB so a batch of different-sized parts all reach the requested size. A single-object selection is the exact previous behaviour — the anchor is then its own translation, so an edit is still an absolute set. The header shows `"N objects"` for a batch and **nothing** for one; it deliberately does not name the file (every duplicate shares a name, so it identified nothing).
- **`preferred_orientation_deg` lives on the printer profile**, not in the plate preferences, because it describes the machine (CoreXY prints everything at 45°). Settings → Printers is the **only** editor; the placement popover shows it read-only and links there, so one machine's angle is never changed from a plate-scoped surface. It rides along inside `orient_options.preferred_z_rotation_deg` and is therefore **only applied when auto-orient runs** — the popover says so rather than showing a live-looking value that does nothing. The CLI's equivalent is `MachineConfig::preferred_print_rotation_deg`, fed in by `SliceCommand::arrange_options`.
- **Plate-editing chrome hides in G-code preview.** The placement control, add-model button, gravity toggle, gizmo-mode group and objects list are all gated on `viewMode() === 'model'`, and the `A` shortcut matches only there. Preview shows toolpaths, so an edit made from it changes something the user cannot see change.
- **The viewer mirrors, it does not own.** `Viewer.syncWasmMeshes()` diffs `sceneEngine.objects()` against its Three.js nodes and adds/disposes to match, so an object created by *anyone* (add button, `Duplicate`, undo) renders without the viewer being told. Do not add a second place that constructs display meshes.
- **`SlicerFile` holds a list, not a file.** `files` accumulates `{fileId, filename}`; `upload(file)` appends and attaches to the open workplate. `fetchFile` adopts a file as the *primary* displayed model (it sets `selectedFile`, which retargets the viewer's `model` input); additional objects must use **`downloadFile`**, which registers without touching `selectedFile` — otherwise restoring an N-object plate leaves only the last file on screen.
- **`toSliceDtos`** ([scene-slice-dto.ts](ui/src/app/runtime/adapters/cloud/scene-slice-dto.ts)) resolves each object to its file via `source_id` and throws rather than guessing. It is a pure function with tests pinning the regression; keep the mapping there, not inline in the runtime adapter.

## Object identity through slicing — exclude-object & sequential printing

[src/core/objects.rs](src/core/objects.rs) is where a *plate* (several placed
objects) becomes *layers* without losing track of which part is which. Issues
#22 (exclude object) and #112 (sequential printing) need exactly the same
segmentation, so it is built once, here.

- **[`slice_plate`](src/core/objects.rs) is the single slicing entry point.**
  CLI, WS server, wasm `web-slicer` and the desktop Tauri bridge all hand it a
  `&[ObjectInput]` instead of merging meshes themselves. Do not re-introduce a
  "just concatenate the faces and call `process_mesh`" site — that is the merge
  that erased object identity in the first place.
- **The merged fast path is not optional.** When
  [`SlicingParams::object_aware()`](src/settings/params.rs) is false (neither
  `exclude_object` nor `print_sequence = by_object`), `slice_plate` merges and
  calls `process_mesh` exactly as before, so the default configuration produces
  **byte-identical G-code**. Object-aware slicing runs the pipeline once *per
  object*, which is **not** output-equivalent: `calculate_interior_region`
  averages the wall-bead count across a layer's islands, so an island's interior
  estimate depends on what else shares its layer. Slicing a part alone is the
  more faithful result, but it is still a change and must only happen when asked
  for. `merged_path_matches_a_plain_process_mesh` pins this.
- **`SliceLayer::path_objects` is a parallel array with an empty sentinel**, like
  `path_overhang`: empty means "not sliced object-aware", and `None` at an index
  means "belongs to no object". Every helper that rebuilds a layer's parallel
  arrays — notably [`adhesion::prepend`](src/adhesion/mod.rs) — must carry it
  along or the tags silently shift onto the wrong paths.
- **Adhesion is plate-wide in `by_layer`, object-owned in `by_object`.** In
  layer order the skirt/brim is generated **once on the merged stack** and
  tagged `None`, so cancelling one part does not take the plate's adhesion with
  it; in object order each object is a self-contained print and owns its own.
- **Layers merge by Z slot, not by index.** Two parts resting on the bed slice
  onto the same grid, but a part lifted off the bed keeps its own (its bottom
  layers are *its* bottom layers). `merge_layers_by_z` groups within a quarter
  layer and takes at most one layer per object per slot, so emitted Z is always
  strictly ascending.
- **Sequential order is front-to-back** (`min_y`, then `min_x`) — the gantry
  sweeps from behind, so finishing the nearest part first keeps the carriage
  away from finished work longest. Clearance problems are **warnings, not
  errors**: the clearances are machine estimates and refusing to slice would be
  worse than saying what to check. Only objects printed *before* another are
  height-checked; the last one has nothing reaching over it.
- **What lives on the printer vs. the process.** Whether the machine *can*
  cancel an object (`exclude_object`) and how much room its printhead needs
  (`extruder_clearance_height_mm` / `extruder_clearance_radius_mm`) are
  properties of the **machine**, so all three carry the **Hardware** `x-group`
  (printer contract), alongside nozzle diameter — mirroring PrusaSlicer's
  Printer Settings and the rationale that put `preferred_orientation_deg` on the
  printer profile. Two printers can run the same `by_object` process yet differ
  in gantry height, duct radius, and firmware object support. Only the
  print-behaviour choices (`print_sequence`, `between_objects_gcode`) are
  process settings, under the **Objects** group. The clearances are intrinsic
  machine specs, so they carry **no** `x-relevant-when` gate (a printer always
  has a clearance) — which also avoids a cross-contract gate pointing at a
  process field in another tab.
- **Object names are sanitised and de-duplicated in the engine.** Klipper parses
  `EXCLUDE_OBJECT_DEFINE NAME=…` as a G-code parameter, so a space splits the
  token, and two parts sharing a name would cancel together. Every runtime feeds
  user-chosen filenames straight in, so `unique_object_name` fixes both centrally
  rather than at each call site.

### Emitting the markers

Three `GcodeDialect` methods carry it into firmware syntax:
`object_definitions`, `object_start`, `object_end`. The **defaults implement
`M486`** (Marlin 2.0.9.3+, RepRapFirmware, Prusa); `KlipperDialect` overrides
them with `EXCLUDE_OBJECT_*`, which additionally carries `CENTER` and `POLYGON`
so the firmware knows where a cancelled object lives.

- **Definitions are emitted before the start script.** Klipper's
  `[exclude_object]` module and Moonraker both expect to meet every object
  before the print begins, so a front-end can list the parts the moment the file
  loads.
- **The marker block switches *before* a path's travel**, so the hop between two
  parts is charged to the one it is heading for — the PrusaSlicer/OrcaSlicer
  convention, and what lets a firmware skip a cancelled object's approach moves
  along with its extrusions.
- **`M486 A"name"` is spent once per object** (`first_use`); repeating the name
  on every layer would bloat the file for no gain.
- **The sequential hand-over happens before the layer's own Z move.** Order is
  load-bearing: close the marker block → retract → lift above the tallest thing
  already printed → travel across → `between_objects_gcode`. The layer block
  that follows then drops to the new object's first layer *over empty bed*.
  Doing any of it afterwards lowers the nozzle into the part just finished.
  Pinned by `sequential_lifts_clear_of_the_finished_object_before_travelling`.
- **The exclusion polygon is a convex hull, resampled to ≤ 64 points.** A turned
  cylinder hulls to one vertex per facet and would push a multi-kilobyte line
  into the file for no added precision.

## Slicing Pipeline — Deep Knowledge

This section records hard-won understanding of how the slicing pipeline works and
why specific design decisions were made. Read this before touching anything in
[src/core/](src/core/) or [src/walls/](src/walls/).

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
path ordering + flow compensation    — greedy-TSP per role group, then wall-overlap flow scaling
apply_adhesion()                     — skirt/brim prepended to first layer(s); raft prepends layers + Z-shifts object (src/adhesion/)
```

Order matters critically. Surfaces are computed **after** Arachne walls so that
`calculate_interior_region` sees the correct bead geometry. Infill is computed
**after** surfaces so it can subtract `solid_regions`. **Bed adhesion runs dead
last**, after ordering and flow compensation, so its loops are appended cleanly
and the object's own toolpaths are provably unperturbed — see
[src/adhesion/README.md](src/adhesion/README.md).

**Perimeter routing & ordering options ([#98](https://github.com/ColdCrabby/slicer/issues/98))** are threaded through several stages:

- `external_perimeters_first` (default `false` = inner walls first, outer wall
  **last** — the PrusaSlicer/Orca/Cura default), `thin_walls` (default `true`),
  and `extra_perimeters` are handled **inside the wall generators** at bead
  assembly. Ordering is a bead-flush concern, not a pipeline pass — the beads are
  computed outer-first then reversed for inner-first; the greedy-TSP path ordering
  preserves the per-role group order. Reordering never changes extrusion amounts,
  so QA baselines are unaffected. See [src/walls/README.md](src/walls/README.md).
- **`thin_walls` is a classic-generator option, and the schema gates it.**
  Arachne fills thin features from the medial axis by construction, so it always
  prints them and ignores the flag (matching PrusaSlicer/Orca, where the
  equivalent option is classic-only); `emit_residual_medial_fill` is
  unconditional. This matters because that pass emits the same `GapFill` role for
  two different things: a bead that *is* the model geometry (a feature too thin
  for one perimeter) and a bead filling the sliver *between* the innermost walls.
  Gating the pass on `thin_walls` deleted both — on the Filament Card Caddy it
  wiped all 37.7 m of gap fill, ~50 card-slot fins, and let sparse infill leak
  into the freed wall band. **Any wall option only one generator honours must
  carry an `x-relevant-when` gate** (`thin_walls` and `wall_distribution_count` →
  classic, `gap_fill_min_length_mm` → arachne), or the UI offers a control that
  silently does nothing.
- `ensure_vertical_shell_thickness` is a **second pass in
  `generate_top_bottom_surfaces_with_interior`** (`apply_vertical_shell_thickness`):
  it grows each layer's own top/bottom surface inward and fills it solid so a
  sloped side wall keeps a continuous perpendicular shell. No-op on flat tops and
  plain vertical walls; default off.
- `avoid_crossing_perimeters` is a **G-code-generation** concern
  ([src/gcode/travel.rs](src/gcode/travel.rs)): a per-layer visibility-graph
  planner detours travel moves around outer walls. It only reshapes travels, so
  extrusion (and QA baselines) are untouched; default off.

**`pre_strip_infill_regions` must be computed before `apply_single_wall_restrictions`.**
`apply_single_wall_restrictions` now operates **per island**: an outer-wall path P at
layer i has its associated inner walls stripped only when P's footprint has an exposed
top surface AND P does not appear in layer i+1 (the island ends here). The large body
island on the same layer is unaffected. The `pre_strip_infill_regions` snapshot is
still taken before this step as a defensive measure — the snapshot preserves the correct
`walls_per_island` count for every island in case future changes ever re-introduce a
layer-wide strip.

### Spiral (Vase) Mode — Normalize in the Pipeline, Spiralize in the Generator

`spiral_vase` prints a single continuous outer wall whose Z ramps over each
layer (a seamless single-wall vase). It is split across two boundaries so every
runtime (CLI / WS / WASM) behaves identically:

- **Normalization** ([`SlicingParams::spiral_vase_normalized`](src/settings/params.rs))
  forces the incompatible settings off — `wall_count = 1`, `infill_density = 0`,
  `top_layers = 0`, `retract_mm = 0`, `z_hop_mm = 0`, `ironing_enabled = false` —
  while **keeping `bottom_layers`** as the solid base. It is a `Cow` (a no-op
  borrow when the flag is off) and **idempotent**, so it is applied at both
  boundaries below without double-effect: at the top of `process_mesh` /
  `process_mesh_debug`, and again at the top of `generate_with_stats`.
- **The pipeline skips `classify_overhang_perimeters` in spiral mode.** That
  pass splits closed wall loops into open arcs; the spiral emitter needs each
  spiral layer's outer wall to stay one closed loop, so it is guarded by
  `!params.spiral_vase`. Nothing else in the pipeline changes — surface
  generation still runs (for the base) and everything is a plain single-wall
  slice.
- **The generator owns the spiralization** ([src/gcode/generator.rs](src/gcode/generator.rs)).
  Spiral layers are those at index `≥ bottom_layers.max(1)` (layer 0 is always
  flat — a spiral cannot climb from Z=0) that expose exactly **one outermost
  closed `OuterWall` loop** (`detect_spiral_loop`: hole sub-loops are ignored by
  a winding-independent point-in-polygon containment test, so a solid island
  with holes still spiralizes as one contour). For those layers the discrete
  per-layer `move_z` is skipped and `emit_spiral_loop` walks the loop once,
  ramping Z from the previous layer's top to this layer's Z in proportion to the
  distance travelled (`move_extrude_z`). Flow **fades in** over the first spiral
  loop and **out** over the last so both ends of the seam disappear — applied as
  a multiplier *after* `extrusion_for_move` (a zero passed as `flow_ratio` trips
  its "non-positive → 1.0" guard). Each loop is rotated to start nearest the
  previous nozzle position to keep the start line aligned and travel minimal.
- **Multi-island layers fall back** to a normal flat print (all paths, discrete
  Z) with a single warning — spiral vase is for solid, single-island models.

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

**Isolated gap-fill "splat" beads are dropped by a `2·d` minimum run length.**
Separate from the *spur* prune above, `emit_medial_beads` and the residual
pre-filter (`emit_residual_medial_fill`) both discard any emitted gap-fill *run*
shorter than [`gap_fill_min_run_len_mm`](src/walls/arachne/generate.rs) —
`gap_fill_min_length_mm` when the user set it (`> 0`), else `2·d` (0.8 mm at a
0.4 mm nozzle). The old auto-default was `d`, which let ~270 sub-`2·d` beads
survive on the 3DBenchy: each an isolated dab of material that still costs a full
retract → travel → un-retract to reach — the "tiny inner-body splat" that wastes
time and grinds filament. The residual such a splat would fill is bridged by the
squish of the flanking wall beads, so `classic` (which has no gap fill) leaves
the same curved/tapering wall corners bead-free with **no** measurable wall-zone
void (`voids.py`). Matching the spur floor is deliberate: a run below the same
`2·d` that separates a real gap spine from facet noise *is* facet noise once
isolated as its own bead.

**Redundant gap fill *under* a solid surface is pruned two ways.**
[`prune_redundant_gap_fill`](src/core/surfaces.rs) drops a `GapFill` bead when
either (1) a majority of its vertices lie **inside** `solid_regions`, or (2) it
is **sandwiched** — solid surface on *both* perpendicular sides
(`gap_fill_sandwiched_by_surface`). Case (2) exists because
`blocked_for_surface` unions the gap-fill footprint *out* of the surface region
(so the surface *abuts* genuine thin necks), which carves a bead-wide corridor
in `solid_regions` exactly where each bead sits — so a bead running down the
centre of a thin solid strip is never "inside" the surface, yet the surface's
full-width rectilinear zig-zag still deposits straight over it. Measured on the
3DBenchy rear rail (≈ layer 200): 6 mm²/layer of `GapFill × TopSurface`
double-extrusion that a footprint-erosion overlap scan (`overlap.py`) *hides*
because the bead is thin — use a true-width capsule intersection to see it. The
sandwich probe reaches `half-width + 0.5·d` to either side, just past the carved
corridor: a bead the surface *surrounds* has surface on both probes and is
dropped; a genuine neck that merely *abuts* a surface edge has it on at most one
and is kept (sparse infill would skip that sub-nozzle channel). Model-wide this
took `GapFill × TopSurface` from 7.3 → 0.5 mm² by pruning ~3 long beads, with no
new wall-zone void where the surface already covers the strip.

**The solid surface must _cover_ a sandwiched gap bead's footprint, not carve
it out.** Pruning the redundant bead (above) is only half the fix: if the
surface region still has the bead-wide corridor carved out of it, that corridor
becomes a **hole** in `solid_regions`. On a thin roof (the 3DBenchy rear rail,
≈ layer 201) the hole (a) splits the top-surface serpentine into **two
disconnected bands** and (b) is re-filled by **sparse-infill dashes** over the
void the pruned bead left — the "two infill surfaces plus tiny blobs of goo"
defect. The corridor was carved from **two** places, so both must stop carving a
sandwiched bead:

- `blocked_for_surface`'s explicit gap-fill term now uses
  `compute_gap_fill_footprint_excluding_sandwiched`, and
- `compute_wall_bead_footprint` — which also lists `GapFill` — is called with
  `include_gap_fill = false` for the surface trim (the walls-only variant),
  because the gap fill is accounted for by that separate, sandwich-aware term.

The sandwich test there runs against the layer's **combined detected surface**
(`combined_surface_region(bridge, bottom, top)`, pre-trim) so a centre bead is
recognised as redundant before the trim would hole the surface. Result: the roof
fills as **one** solid region, the bead is pruned, and no sparse infill leaks in.
Genuine one-sided necks are still carved (surface abuts, never welds). Verify
with a true-width capsule render (grey wall + one red top surface, no green bead,
no orange dash) and the debug SVG (`--debug-geometry`): the rail `solid_surface`
must be **one CCW polygon**, not a CCW ring with a CW hole.

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

`walls_per_island = ceil(total_wall_bead_count / outer_contour_count)` estimates the
number of wall shells per island. **This is only an average, and the Arachne
generator breaks the assumption behind it:** Arachne places a _variable_ number
of variable-width beads per island (and even along one island), so on a layer
whose islands differ in bead count the estimate is too low and the interior is
under-deflated by up to a full bead width. Solid surface / sparse infill / bottom
fill generated inside that over-reaching interior then lands _on top of the
innermost wall_ (measurable "Inner wall × {Top surface, Sparse infill, Bottom
surface}" double-extrusion; the classic generator, which places a fixed count,
does not show it). See the wall-footprint clip below for the correction.

**Do not normalise all wall paths to CCW before the inflate.** Hole boundary
beads have CW winding. Flipping them to CCW makes Clipper2 treat holes as solid
material → infill is generated inside the hole (through the void).

### Wall-Footprint Clip — Correcting the Interior Estimate's Over-Reach

Because the `walls_per_island` estimate above can under-deflate on Arachne
layers, both the sparse-infill area (`add_infill_to_layers`) and the solid
top/bottom surface fill (`generate_top_bottom_surfaces_with_interior`) are
additionally clipped against the **actual physical wall-bead footprint**
(`compute_wall_bead_footprint` — every wall centerline inflated by its own
half-width). This is count- and width-agnostic and a **no-op where the estimate
was already correct** (classic), so it removes only the genuine over-print:

- **Sparse infill** subtracts the footprint _grown by_ `infill_perimeter_gap_mm`
  so it keeps its intended clearance from the real innermost wall.
- **Solid surfaces** subtract the footprint _eroded by_
  `infill_overlap_percent × d` so the fill still welds that much into the
  innermost wall (the designed bond) and no further. The un-eroded gap-fill
  footprint is unioned back in so surfaces _abut_ gap fill rather than welding to
  it.

Use **`FillRule::NonZero`** for these subtractions, **not `Positive`**: the wall
footprint is a frame with CW hole sub-paths (the enclosed interior). `Positive`
ignores CW holes → treats the frame as a solid block → erases the whole interior.

This clip is applied to the fill regions **only** — it never reshapes
`interior_regions`, so bridge detection/classification (which keys off the
smooth interior) is untouched. Reshaping the interior itself instead spawns
phantom bridges from the jagged bead-following boundary.

Trimming the surface off the wall band shrinks `solid_regions`, so
`prune_redundant_gap_fill` correctly _retains_ the wall-band gap-fill beads it
used to prune (they fill real medial gaps, not areas the surface covers) — this
is why the Arachne `gap_fill` role length rises after the fix.

### Infill Boundary vs. Surface Region

`add_infill_to_layers` calls `calculate_interior_region(layer, 0.0, nozzle_diameter_mm)`
(overlap = 0) to get the infill area, then subtracts `layer.solid_regions` with
`FillRule::Positive` and finally the wall-bead footprint (see above) with
`FillRule::NonZero`.

`generate_top_bottom_surfaces_with_interior` clips surface regions to
`interior_regions[i]` (computed ahead of time with
`calculate_interior_region(layer, infill_overlap_percent, nozzle_diameter_mm)`)
then subtracts the eroded wall-bead footprint before generating solid infill
lines.

**`solid_regions` is grown by one bead before being subtracted from the
sparse-infill area.** `solid_regions` is a nominal polygon, but the surface is
actually printed as a rectilinear serpentine whose **stepped extent** only
approximates it, and the surface pass has already trimmed it back off the wall
band. Subtracting the raw outline therefore leaves a thin crescent **sliver**
between the solid region and the wall all along a curved perimeter (plainly
visible on the 3DBenchy hull, layers ≈ 40–42). The scanline shatters that sliver
into a swarm of sub-millimetre dashes — 31 on layer 41 alone — each an isolated
dab costing a full retract → travel → un-retract. That cycle pushes ~4.8 mm³ of
filament back and forth through the nozzle to deposit ~0.04 mm³: the "infill
produces tiny extrudes" defect, pure waste and a grinding risk, with zero
structural gain (the space is already flanked by the solid surface on one side
and a wall bead on the other).

`add_infill_to_layers` therefore subtracts `inflate(solid_regions,
SOLID_MARGIN_NOZZLE_MULT × nozzle)` — one bead width, since the surface's own
fill lines are `d` wide about their centerlines and the nominal polygon
under-states the deposited material by up to half a bead per side. Measured on
3DBenchy layer 41: isolated sub-1.5 mm infill paths fall 33 → 6 at `1.0`, while
`0.5` leaves all 33 (the sliver is simply wider than half a bead).

**Key this correction to `solid_regions`, never to the infill area as a whole.**
Doing so makes it an exact **no-op on layers with no solid surface**, so a
genuinely thin wall-to-wall cavity keeps its full sparse lattice. A first
attempt instead morphologically **opened** the whole infill area to erase
channels narrower than `2.5 × nozzle`. That cannot distinguish an artifact
sliver from a real thin cavity: it erased the filament caddy's hollow-box
lattice outright — wall-zone void more than doubled (62 → 146 mm²) and 35 % of
its infill vanished — and the **slicing quality gate caught it**
(`caddy/classic: role infill 13170.1 → 8505.5`). A sweep confirmed there is no
safe threshold for that approach: even `1.0 × nozzle` destroyed the caddy
lattice while artifacts only cleared at `≥ 1.5`. The regression test
`test_thin_cavity_without_solid_surface_keeps_its_infill` pins the correct
behaviour.

`min_infill_extrusion_mm` still guards the residual sub-threshold segments a
legitimate region's tapering corners produce.

**A connected infill region too small to hold more than one dash is skipped
entirely** (`INFILL_MIN_REGION_AREA_NOZZLE_MULT × d²`, = 2.0 mm² at a 0.4 mm
nozzle). Where a cross-section is *locally* thinner than the per-island average
`walls_per_island`, the interior estimate leaves a small sliver that walls plus
gap fill already fill — the Benchy bow tip (≈ 1.6 mm² at layer 95) is the
canonical case. The scanline drops a single ~1.3 mm dash into it: a disconnected
speck costing a full retract → travel → un-retract, sitting right against the
wall band.

Two properties make this safe, and both must be preserved:

- **It is an _area_ rule on whole connected regions, never a _width_ rule on the
  infill area.** A genuinely thin cavity that deserves a lattice (the caddy's
  hollow-box layers) is a *large* region that merely happens to be narrow. The
  separation is categorical, not marginal: measured across the QA corpus the
  caddy has **no** infill region at all between 0.01 mm² and 10 mm², so the
  threshold sits in an empty band two orders of magnitude wide. This is the same
  trap the morphological-opening attempt fell into (see above).
- **It filters the _generated paths_, not the region.** `generate_rectilinear_infill`
  seeds its scanline phase from the bounding box of the whole infill area, so
  deleting an outlying sliver *before* generation shifts every infill line on the
  layer (measured: 27 mm of line movement on a Benchy layer whose dropped regions
  totalled 0.05 mm²). Filtering afterwards is exactly subtractive — measured
  −3.6 mm of sparse infill model-wide on the Benchy, with every other role
  untouched and the caddy byte-identical.

Membership is tested with **segment midpoints**, not vertices: an infill line's
endpoints lie exactly *on* the region boundary, where the integer-scaled
point-in-polygon test can land either side.

**Note:** gap-fill length is not bit-reproducible between runs of the same
binary (measured 7399.6 vs 7401.8 mm on two Benchy slices), so small gap-fill
deltas are run-to-run noise, not evidence of a change. Sparse infill *is*
deterministic and can be compared directly.

### Thin Wall-Band Channels — Opened-Interior Surface Clip

`calculate_interior_region` uses a **per-island _average_** wall count
(`walls_per_island = ceil(total_wall_beads / outer_islands)`), so wherever a
cross-section is _locally_ thinner than that average — the Benchy hull-side wall
tips, the funnel-to-roof transitions, the cabin roof-ridge line, embossed
calibration-cube logos — the interior estimate leaves a **thin sliver channel**
(≤ ~1 mm). Arachne already fills that channel solid with wall + gap-fill beads,
but wherever the geometry above (top) or below (bottom) recedes layer-over-layer
the channel gets picked up as an "exposed" surface and filled with a
**rectilinear zig-zag of sub-millimetre segments** — the "tiny extrudes that
make no sense / weird top-surface spots on the sides" defect. The `classic`
generator, whose uniform offsets fully consume the same cross-sections, emits
_no_ surface there.

Fix (`generate_top_bottom_surfaces_with_interior`): the top/bottom surface is
clipped to a **morphologically _opened_ interior** — `open_interior_for_surface`
erodes then dilates `interior_regions[i]` by
`SURFACE_MIN_INTERIOR_WIDTH_NOZZLE_MULT × nozzle / 2` (2.5× → erase channels
< 1.0 mm at 0.4 mm) — instead of the raw interior. A surface that lands entirely
inside a thin channel therefore disappears, while genuine surfaces (deck, roof,
funnel cap, cube top) keep their **full extent** (only their corners, which sit
inside the walls, are rounded). Key points:

- **The discriminator is the _interior_, not the strip.** A real fore-deck top
  surface is an equally thin band; what separates it from an artifact is that it
  sits on a _thick_ interior (real infill area) rather than a _thin_ wall-band
  channel. Filtering the strip by its own width wrongly deletes legit thin
  surfaces — do not do that.
- **Dropped strips stay solid** via the wall + gap-fill beads that already fill
  them, and — no longer being a `solid_region` — their gap-fill beads survive
  `prune_redundant_gap_fill` (so the wall channel is filled with a proper
  single centered bead instead of the zig-zag). Expect a small **gap_fill ↑ /
  top+bottom_surface ↓** shift in the QA baselines.
- **The absolute base cap (`i < bottom_layers`) is exempt** for bottom surfaces:
  it is the bed-contact region and must stay fully solid for adhesion, so it
  clips to the _full_ interior. Tapering artifacts only occur mid-model.
- **Bridges get the same opened-interior clip** (in `clip_to_void`, step A —
  `intersect(candidate, open_interior_for_surface(interior_regions[i]))`). The
  identical per-island-average under-deflation that spawns phantom _surfaces_
  in a thin channel also fires a phantom **bridge** there: on the Benchy
  hull-side deck edge and sloped cabin front (Arachne layers ≈ 159–172) the
  wall leans past ~45°, a thin unsupported strip survives the d/2 support
  envelope, and — because Arachne's average leaves a non-empty sliver interior
  — the old raw-interior clip let a bridge fire and lay sparse lines straight
  over the wall + gap-fill beads that already fill the channel (measured
  `Gap infill × Bridge` and `Inner wall × Bridge` double-extrusion, absent from
  `classic`). Opening the interior erases the sub-1 mm channel so the phantom
  bridge vanishes and the strip stays solid via its walls + gap fill, while a
  **genuine** bridge over a _wide_ void (cabin roof, porthole / window / door-
  header closure) sits on a _thick_ interior and keeps its full extent. The
  wall-bead-footprint subtraction (step B) is unchanged. Bridges remain exempt
  from any clip that would reshape `interior_regions` itself — only the bridge
  _candidate_ is clipped, never `bridge_region` detection input, so bridge
  classification is untouched.
- Verified against `classic` with [tools/gcode-analysis/](tools/gcode-analysis/README.md):
  the removed strips open no new wall-zone void (`voids.py`) and cross-role
  double-extrusion drops (`overlap.py`). After the bridge clip, every
  `… × Bridge` overlap pair on the Benchy matches `classic` to within noise
  (`Gap infill × Bridge` 8.6 → 0, `Inner wall × Bridge` 8.3 → 0.16 = classic).

### Sub-Bead Slivers — Grazing-Angle Surface Fill

The wall-band trim above subtracts the eroded wall-bead footprint from a surface
region whose outline does **not** follow that footprint exactly. Where the two
boundaries meet at a **grazing angle** the subtraction leaves a long crescent far
narrower than one bead. The scanline still fills it, but because the fill
direction is then near-parallel to the crescent **every span is a stub**.

Measured on the Filament Card Caddy's hexagon logo (0.44 mm extrusion,
`wall_count = 3`): the `solid_surface` region carried separate sliver sub-paths
of ≈4.5 mm² and ≈0.22 mm mean width along exactly the two hexagon edges lying
15° off the fill direction. A 135° scanline crossing a 0.22 mm strip at 150°
gives span `0.22/sin 15° ≈ 0.85 mm` — matching the observed repeating
**0.82 mm line / 0.62 mm connector** micro-serpentine. **93 % of that material
was already covered** by the flanking wall bead or the normal surface, so it
bought ≈0.5 mm² of real coverage for ~20 mm of sub-millimetre moves.

`open_surface_region_for_fill` (applied to `bottom_region` / `top_region` after
the wall-band trim, before `add_solid_infill_for_region`) removes them:

- **Threshold is physical, not heuristic.** Erode by
  `SURFACE_FILL_MIN_WIDTH_FRACTION (0.5) × solid-surface extrusion width`, i.e.
  an erosion *diameter* of exactly one bead. A strip narrower than one bead
  cannot hold a bead by construction.
- **It is a _width_ filter, not an area filter.** Small-but-printable surfaces
  survive intact (measured: 79 mm² and 37 mm² regions untouched while six
  slivers went to zero).
- **Corners are preserved.** A plain morphological opening rounds convex
  corners, and a rounded corner makes the scanline emit *extra* stub spans —
  the very artifact being removed (measured +31 stubs on the Voron cube). So
  the surviving core is re-grown by `SURFACE_FILL_REGROW_FACTOR (2.0) × radius`
  and **clipped back to the original region**, restoring exact original shape.
  That took the cube from +31 stubs to −1.
- Use **`FillRule::NonZero`** for the final clip so CW hole sub-paths stay holes.
- This defect is **not** Arachne-specific — `arachne` and `classic` produced
  byte-identical stub measurements on the caddy hexagon, because it originates
  in the surface fill, not the wall generator.

Measured effect (user profile, whole model): coverage change ≤ 0.004 % on
Benchy / Voron cube / caddy, with sub-1 mm segments −19 / −1 / −55 and the
caddy's two grazing edges dropping **22.9 → 2.4 mm** and **24.1 → 2.8 mm** of
sub-1 mm top surface. QA gate passes with **no baseline drift**.

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
- [SETUP.md](SETUP.md) - Prerequisites and first-run steps
- [docs/use/](docs/use/) - End-user documentation
- [docs/teams/](docs/teams/) - Self-hosting, configuration, automation
- [tools/gcode-analysis/](tools/gcode-analysis/README.md) - G-code quality diagnostics (wall overlap, unfilled gaps, bead widths, render/zoom)
- [Clipper2 Documentation](https://github.com/AngusJohnson/Clipper2) - Polygon clipping reference

---

**Last Updated**: 2026-08-23 (versioning + changelog + GitHub Releases)  
**Maintainer Guidance**: Keep this file in sync with project structure changes, new conventions, or significant architectural decisions.
