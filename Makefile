.PHONY: build build-release build-windows build-macos build-wasm clean test fmt lint help changelog-draft

help:
	@echo "Slicer Engine - Build Targets"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@echo "  build              - Debug build (native)"
	@echo "  build-release      - Release build (native)"
	@echo "  build-windows      - Build for Windows (x86_64)"
	@echo "  build-macos        - Build for macOS (x86_64 and ARM64)"
	@echo "  build-wasm         - Build for WebAssembly"
	@echo "  test               - Run tests"
	@echo "  fmt                - Format code"
	@echo "  lint               - Run clippy linter"
	@echo "  changelog-draft    - Draft CHANGELOG notes from git history"
	@echo "  clean              - Clean build artifacts"

build:
	cargo build --verbose

build-release:
	cargo build --release --verbose

build-windows:
	cargo build --release --target x86_64-pc-windows-msvc --verbose

build-macos:
	cargo build --release --target x86_64-apple-darwin --verbose
	cargo build --release --target aarch64-apple-darwin --verbose

build-wasm:
	wasm-pack build --target web --release --out-dir ui/src/generated/scene-wasm --out-name scene_engine

test:
	cargo test --verbose

fmt:
	cargo fmt

lint:
	cargo clippy --all-targets --all-features -- -D warnings

changelog-draft:
	@bash scripts/gen-changelog-draft.sh

clean:
	cargo clean
	rm -rf pkg/
