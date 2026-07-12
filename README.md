# Slicer Engine

🌐 **[Try the online slicer](https://max-scopp.github.io/slicer-engine/)** → no install, no account, works right now.

**Slice your 3D models instantly — in your browser, on your desktop, or on your own server. Your workflow, your choice.**

Drop in an STL, OBJ, or 3MF and get print-ready G-code in seconds. One engine, three ways to run it:

|                    | Where it runs                             | Setup                                                                   |
| ------------------ | ----------------------------------------- | ----------------------------------------------------------------------- |
| 🌐 **Web**         | Fully in your browser — nothing installed | None — [just open the link](https://max-scopp.github.io/slicer-engine/) |
| 🖥️ **Desktop**     | Native app, runs entirely on your machine | Download & run                                                          |
| ☁️ **Self-hosted** | Host it yourself, share with your team    | `cargo run -- serve`                                                    |

Every mode uses the same slicing engine, so results are identical regardless of how you run it. In the browser, your files never leave your machine.

📖 **Full documentation: [https://max-scopp.github.io/slicer-engine/docs/](https://max-scopp.github.io/slicer-engine/docs/)** — architecture, module guides, and contributor docs.

---

## Prerequisites

Before building or running, ensure you have:

### Required

- **Rust 1.70+** — [Install](https://rustup.rs/)
- **Node.js 20+** and **pnpm 9+** — [Node](https://nodejs.org/), then `npm install -g pnpm`

### For WASM builds (browser slicer, cloud UI)

Add the WebAssembly target and install wasm-pack:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```

### For desktop app builds

Install Tauri CLI (choose one):

```bash
cargo install tauri-cli --version "^2"
# OR: pnpm add -g @tauri-apps/cli
```

### Optional

- **C++ toolchain** (clang++ or MSVC) — needed only for full WASM builds with polygon clipping support
  - Linux: `sudo apt install build-essential clang`
  - macOS: `xcode-select --install` (Xcode Command Line Tools)
  - Windows: Visual Studio or Build Tools

---

## Quick Start

```bash
# Slice an STL to G-code
cargo run --release -- slice --input model.stl --output output.gcode

# Run the WebSocket + UI server (default port 5201)
cargo run --release -- serve

# Inspect or edit persisted settings
cargo run --release -- settings show
cargo run --release -- settings set layer_height 0.15
```

---

## Architecture at a glance

```mermaid
graph TB
    subgraph Surfaces
        F["CLI"]
        S["WebSocket server"]
        subgraph UI["Angular UI"]
            CM["cloud mode<br/>(scene in WASM,<br/>slice on server)"]
            WM["web mode<br/>(scene + slice<br/>in WASM)"]
            NM["native mode<br/>(Tauri desktop,<br/>all in Rust)"]
        end
    end

    subgraph Core["Rust core"]
        SC["scene/<br/>SSOT for placement"]
        M["mesh/"]
        SL["core/<br/>slicing pipeline"]
        A["arachne/<br/>walls"]
        I["infill/"]
        G["gcode/"]
    end

    F --> SC
    CM -->|"WS + HTTP"| S
    WM -->|wasm-bindgen| SC
    NM -->|"wasm-bindgen scene"| SC
    NM -->|"tauri::invoke slicing"| SC
    S --> SC
    SC --> M --> SL --> A
    SL --> I
    SL --> G

    style SC fill:#fff9c4
    style SL fill:#c8e6c9
    style G fill:#e1f5ff
```

The same engine runs in three different environments — on a server, compiled into the browser, and bundled into the desktop app — so slicing results are always identical regardless of where you run it.

The UI selects its **runtime mode** at startup:

| Mode     | Where slicing happens | When               |
| -------- | --------------------- | ------------------ |
| `cloud`  | On your server        | Default web build  |
| `web`    | In your browser       | `web-slicer` build |
| `native` | On your desktop       | Desktop app        |

See [Scene Engine](src/scene/README.md) and [Slicing Pipeline](src/core/README.md) for the contract.

---

## Configuration

Slicer Engine is configured via [`slicer.toml`](src/config/README.md). Resolution order:

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
slicer-engine config show
slicer-engine config set slicing.layer_height 0.15
slicer-engine slice --input model.stl --config ./slicer.toml
```

Full reference → [Settings](src/settings/README.md) · [Config (TOML)](src/config/README.md) · [CLI](src/cli/README.md).

---

## Self-hosted web UI

```bash
# 1. Build WASM scene bindings
pnpm run hydrate            # wasm-pack + schema/type gen

# 2. Start dev servers (both must run)
pnpm run ui:dev             # Angular dev server → http://localhost:4200
cargo run --release -- serve # WebSocket/HTTP server → http://localhost:5201
```

The UI sends slicing jobs to the local server. Scene management runs in the browser for instant feedback.

---

## Browser slicer (no server needed)

> **Live demo:** [https://max-scopp.github.io/slicer-engine/](https://max-scopp.github.io/slicer-engine/) — slice in your browser, no backend required.

The full slicing pipeline runs in-browser. Building this locally requires a wasm-capable C++ toolchain (`clang++`) for the polygon clipping library.

```bash
# Build the full WASM bundle (scene + slicer)
pnpm run hydrate:web-slicer

# Dev server — no backend required
pnpm run ui:dev:web-slicer   # http://localhost:4200

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
pnpm run desktop:dev

# Production build (outputs a platform installer)
pnpm run desktop:build
```

The desktop app automatically uses the bundled native engine for slicing, giving you full offline capability and the best performance. Scene management is shared with the browser UI, so the experience is identical.

---

## Building

### Native (your host platform)

```bash
cargo build --release                   # Single command — that's it
```

### Cross-platform

```bash
cargo build --release --target x86_64-pc-windows-msvc       # Windows
cargo build --release --target x86_64-apple-darwin          # macOS Intel
cargo build --release --target aarch64-apple-darwin         # macOS ARM
```

### WebAssembly (browser slicer)

Requires: `rustup target add wasm32-unknown-unknown` and `cargo install wasm-pack`

```bash
wasm-pack build --target web --release
```

Or use the pnpm script (which handles schema generation too):

```bash
pnpm run hydrate               # Scene + type bindings
pnpm run hydrate:web-slicer    # Full WASM slicer (includes polygon clipping)
```

### Using Makefile (Linux/macOS)

```bash
make build-release  build-windows  build-macos  build-wasm
```

---

## Troubleshooting Setup

**`wasm32-unknown-unknown` target not found?**

```bash
rustup target add wasm32-unknown-unknown
```

**`wasm-pack` command not found?**

```bash
cargo install wasm-pack
```

**`wasm-bindgen` command not found?**

```bash
cargo install wasm-bindgen-cli
```

**pnpm hydrate fails with C++ compilation errors?**
Install the C++ toolchain for your platform (see Prerequisites above), then retry.

---

## Development

```bash
cargo build                                                 # fast iteration (debug)
cargo test
cargo fmt && cargo clippy --all-targets --all-features -- -D warnings
pnpm --filter slicer-engine-docs docs:dev                   # live docs site
sea-orm-cli migrate generate "my_migration" -d src/db       # scaffold DB migration
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for workflow, [AGENTS.md](AGENTS.md) for AI-agent guidance, and [ARCHITECTURE.md](ARCHITECTURE.md) for the long-form architecture overview (also rendered on the [docs site](https://max-scopp.github.io/slicer-engine/docs/guide/architecture)).

---

## References

[RepRap G-code Wiki](https://reprap.org/wiki/G-code) · [Arachne Paper](https://github.com/Ultimaker/CuraEngine/blob/main/docs/arachne.md) · [Clipper2](https://www.angusj.com/clipper2/Docs/Overview.htm) · [Marlin G-code](https://marlinfw.org/meta/gcode/) · [Klipper G-code](https://www.klipper3d.org/G-Codes.html) · [Tauri](https://v2.tauri.app/)

---

## Implementation notes

Built on proven approaches from established slicers, but written from scratch in Rust. AI tools assist with development and problem-solving; all AI-generated code is reviewed and approved by human maintainers before merge.

---

## License

All rights reserved until an official license is decided. No use, reproduction, modification, or distribution permitted without written authorization. TBD.

---

## Support

[Issues](https://github.com/max-scopp/slicer-engine/issues) · [Discussions](https://github.com/max-scopp/slicer-engine/discussions) · [Contributing](CONTRIBUTING.md) · [Documentation site](https://max-scopp.github.io/slicer-engine/docs/)
