# Building from source

> Prerequisites (Rust, Node, WASM toolchain) are covered in [SETUP.md](SETUP.md).

## Native (your host platform)

```bash
cargo build --release                   # Single command — that's it
```

## Cross-platform

```bash
cargo build --release --target x86_64-pc-windows-msvc       # Windows
cargo build --release --target x86_64-apple-darwin          # macOS Intel
cargo build --release --target aarch64-apple-darwin         # macOS ARM
```

## WebAssembly (browser slicer)

Requires: `rustup target add wasm32-unknown-unknown` and `cargo install wasm-pack`

```bash
wasm-pack build --target web --release
```

Or use the pnpm script (which handles schema generation too):

```bash
pnpm run hydrate               # Scene + type bindings
pnpm run hydrate:web-slicer    # Full WASM slicer (includes polygon clipping)
```

## Using Makefile (Linux/macOS)

```bash
make build-release  build-windows  build-macos  build-wasm
```
