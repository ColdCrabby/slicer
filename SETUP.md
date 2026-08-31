# Setup & running

Everything you need to build and run Cold Crabby from source — as a self-hosted web UI, an in-browser slicer, a desktop app, or on an iPad.

> **Just want to slice?** No setup needed → **[slicer.maxscopp.de](https://slicer.maxscopp.de/)**.
> **Want to know how to *use* it?** → **[Getting started](https://slicer.maxscopp.de/docs/use/)**.
> **Deploying it for a team?** → **[For teams](https://slicer.maxscopp.de/docs/teams/self-host)**.

---

## Prerequisites

Before building or running, ensure you have:

### Required

- **Rust 1.70+ via [rustup](https://rustup.rs/)** — the Homebrew `rust` package is **not supported**; it ships without the `wasm32-unknown-unknown` standard library and can't add targets. If you have it installed, run `brew uninstall rust` first, then install rustup.
- **Node.js 20+** and **pnpm 9+** — [Node](https://nodejs.org/), then `npm install -g pnpm`

### For WASM builds (self-hosted UI, browser slicer, desktop)

All three modes need the WASM scene bindings. Add the target and install a matching `wasm-bindgen-cli`:

```bash
rustup target add wasm32-unknown-unknown

# Match the wasm-bindgen version pinned in Cargo.lock — mismatched CLI
# versions silently produce broken bindings.
cargo install wasm-bindgen-cli --version "$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | tail -1 | cut -d'"' -f2)" --locked
```

> Only needed if you build the standalone in-browser bundle yourself: `cargo install wasm-pack`. The `pnpm hydrate` scripts do not use it.

### For desktop app builds

Install Tauri CLI (choose one):

```bash
cargo install tauri-cli --version "^2"
# OR: pnpm add -g @tauri-apps/cli
```

### For iPad / iOS builds (macOS only)

The full **Xcode** app is required — Xcode Command Line Tools do not ship the iOS SDK. Installing it is not enough; you must also point the toolchain at it:

```bash
sudo xcode-select -s /Applications/Xcode.app
xcodebuild -downloadPlatform iOS    # simulator runtime, ~8 GB, not bundled with Xcode
pnpm run ios:setup                  # verifies everything, installs the Rust iOS targets and CocoaPods
```

Run `pnpm run ios:doctor` at any time for a read-only report of what is missing.

### Optional

- **C++ toolchain** (clang++ or MSVC) — needed only for full WASM builds with polygon clipping support
  - Linux: `sudo apt install build-essential clang`
  - macOS: `xcode-select --install` (Xcode Command Line Tools)
  - Windows: Visual Studio or Build Tools

---

## Quick Start (CLI)

```bash
# Slice an STL to G-code
cargo run -- slice --input model.stl --output output.gcode

# Inspect or edit persisted settings
cargo run -- settings show
cargo run -- settings set layer_height 0.15
```

To run the WebSocket + UI server (`cargo run -- serve`), see **[First-time setup](#first-time-setup-self-hosted-ui)** below — the UI must be built before the server can serve it.

---

## First-time setup (self-hosted UI)

After cloning, run these once in order. Skipping any step means the next one fails with confusing errors.

```bash
# 1. Install deps (Rust + Node prerequisites above must already be in place)
pnpm install

# 2. Build WASM scene bindings + generate JSON schemas + TS types
#    + the Preset Cloud API client (from the remote OpenAPI document).
#    Populates ui/src/generated/ — without this, `pnpm ui:build` fails with
#    "Cannot find module '../../generated/scene-wasm/scene_engine'".
pnpm run hydrate

# 3. Build the Angular UI
#    Produces ui/dist/slicer-ui/browser — without this, `cargo run -- serve`
#    fails with "UI directory not found: ./ui/dist/slicer-ui/browser".
pnpm run ui:build

# 4. Start the server (serves the UI at http://localhost:5201/)
cargo run -- serve
```

For iterative UI work, use the dev-server flow in [Self-hosted web UI](#self-hosted-web-ui) below (skips step 3, hot-reloads Angular).

---

## Configuration

Cold Crabby is configured via [`slicer.toml`](src/config/README.md). Resolution order:

1. CLI flags (per invocation, never persisted)
2. Project config — `./slicer.toml` in the working directory
3. User config — platform path (e.g. `~/.config/slicer-engine/slicer.toml`)
4. Built-in defaults

```toml
[machine]
nozzle_diameter = 0.4
build_volume_x = 220.0

[slicing]
layer_height = 0.2
wall_count = 3
infill_density = 0.20

[server]
port = 5201
```

Manage it from the CLI:

```bash
slicer-engine config show                       # the fully merged result
slicer-engine config init                       # write a starter ./slicer.toml
slicer-engine settings set layer_height 0.15    # change one value
slicer-engine slice --input model.stl --config ./slicer.toml
```

Full reference → [Settings](src/settings/README.md) · [Config (TOML)](src/config/README.md) · [CLI](src/cli/README.md).

---

## Self-hosted web UI

Production flow (server serves the built UI on a single port):

```bash
pnpm run hydrate             # once (or when Rust bindings/schemas change)
pnpm run ui:build            # once (or after UI changes)
cargo run --release -- serve # http://localhost:5201/
```

Dev flow (Angular hot-reload, engine runs alongside it):

```bash
pnpm run hydrate             # once (or when Rust bindings/schemas change)
pnpm run dev                 # engine + UI on a seeded pair of ports
```

`pnpm run dev` rolls a random three-digit **seed** and derives every port from
it — the UI on `4<seed>`, the engine on `5<seed>` — so several checkouts (or
several people on one box) can run at the same time without colliding. It prints
the URL to open:

```
Seed 742
  UI      http://localhost:4742/   <- open this
  Engine  http://127.0.0.1:5742/  (proxied at /api and /ws)
```

Open the UI URL only: the dev server proxies `/api` and `/ws` to the engine, so
the browser talks to a single origin (as it does in production). Pass
`--seed 742` to pin one, `--ui-only` / `--backend-only` to start half the stack,
or `--print` to see the ports without starting anything:

```bash
pnpm run dev -- --seed 742
```

The UI sends slicing jobs to the local engine. Scene management runs in the
browser for instant feedback.

---

## Browser slicer (no server needed)

> **Live demo:** [https://slicer.maxscopp.de/](https://slicer.maxscopp.de/) — slice in your browser, no backend required.

The full slicing pipeline runs in-browser. Building this locally requires a wasm-capable C++ toolchain (`clang++`) for the polygon clipping library.

```bash
# Build the full WASM bundle (scene + slicer)
pnpm run hydrate:web-slicer

# Dev server — no backend required (seeded port, printed on start)
pnpm run dev:web-slicer

# Production build
pnpm run ui:build:web-slicer
```

---

## Desktop app

Bundles the UI and the full slicing engine into a native desktop application. No server required.

```bash
# Prerequisites: install Tauri CLI
cargo install tauri-cli --version "^2"
# or: pnpm add -g @tauri-apps/cli

# Dev mode (hot-reloads Angular, rebuilds Rust on change)
pnpm run dev:desktop

# Production build (outputs a platform installer)
pnpm run desktop:build
```

The desktop app automatically uses the bundled native engine for slicing, giving you full offline capability and the best performance. Scene management is shared with the browser UI, so the experience is identical.

---

## iPad / iOS app

The same Tauri shell also builds for iPadOS and iOS, running the full Rust slicing engine on-device.

**Requires macOS with the full [Xcode](https://apps.apple.com/app/xcode/id497799835) app** — Command Line Tools do not include the iOS SDK. Everything else the doctor script installs or explains:

```bash
sudo xcode-select -s /Applications/Xcode.app   # installing Xcode does not do this for you
xcodebuild -downloadPlatform iOS               # simulator runtime (~8 GB), sold separately
pnpm run ios:setup     # check the toolchain, install Rust iOS targets
pnpm run hydrate       # WASM scene bindings (once, as for every other surface)
pnpm run ios:init      # generate the Xcode project
pnpm run ios:dev       # build + run on an iPad simulator, with live reload
```

`pnpm run ios:doctor` reports without changing anything, and names the exact command to fix whatever is missing.

To keep the app on a real iPad — no dev server, no paid Apple Developer Program — build the standalone app and install it over the pairing you already have:

```bash
pnpm run ios:install
```

A free Apple ID signs for seven days; re-run it (`-- --renew`) to reset the clock. Your models, profiles and settings survive the reinstall.

Full walkthrough, device selection, physical-device signing, and troubleshooting → **[ui-desktop/README.md](ui-desktop/README.md)**.

---

## Troubleshooting setup

**`cargo run -- serve` fails with `UI directory not found: ./ui/dist/slicer-ui/browser`?**
Build the UI first: `pnpm run hydrate && pnpm run ui:build`. See [First-time setup](#first-time-setup-self-hosted-ui).

**`pnpm ui:build` fails with `Cannot find module '../../generated/scene-wasm/scene_engine'` (or `.../slicer-engine-ws-*-message-v1`)?**
You skipped `pnpm run hydrate`. Run it, then retry the build.

**`wasm32-unknown-unknown` target not found / `error[E0463]: can't find crate for 'core'`?**
Either the target isn't installed (`rustup target add wasm32-unknown-unknown`) **or** you're on Homebrew Rust, which can't add targets. Run `brew uninstall rust` and install [rustup](https://rustup.rs/).

**`wasm-bindgen` command not found?**
Install the CLI at the exact version pinned in `Cargo.lock` (mismatched versions produce silently broken bindings):

```bash
cargo install wasm-bindgen-cli --version "$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | tail -1 | cut -d'"' -f2)" --locked
```

Still "command not found" after installing? `cargo install` puts binaries in `~/.cargo/bin`, which `rustup`'s own shell setup should have added to `PATH` — if it didn't (or you're on a shell profile rustup didn't touch), add it yourself:

```bash
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc   # or ~/.bashrc
```

Open a new terminal (or `source` the file) and retry.

**`wasm-pack` command not found?**
Only needed for the standalone `wasm-pack build` invocation — the `pnpm hydrate` scripts don't use it. If you actually need it: `cargo install wasm-pack`.

**pnpm hydrate fails with C++ compilation errors?**
Install the C++ toolchain for your platform (see [Prerequisites](#prerequisites) above), then retry.

---

See also → [Building from source](BUILDING.md) · [Development](DEVELOPMENT.md) · [Architecture](ARCHITECTURE.md)
