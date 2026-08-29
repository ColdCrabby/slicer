.PHONY: build build-release build-windows build-macos build-wasm clean test fmt lint help changelog-draft ios-doctor ios-setup ios-init ios-simulator ios-dev ios-build

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
	@echo ""
	@echo "iOS / iPadOS (macOS host with Xcode):"
	@echo "  ios-doctor         - Check the iOS toolchain and report what is missing"
	@echo "  ios-setup          - Same, and install what can be automated"
	@echo "  ios-init           - Generate the Xcode project (ui-desktop/src-tauri/gen/apple)"
	@echo "  ios-simulator      - Boot an iPad simulator"
	@echo "  ios-dev            - Run the app on an iPad simulator with live reload"
	@echo "  ios-build          - Build a release .ipa"

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

# ── iOS / iPadOS ──────────────────────────────────────────────────────────────
# The Rust side is driven by the Tauri CLI, which owns the generated Xcode
# project; these targets are thin aliases over the pnpm scripts so `make` and
# `pnpm run` stay interchangeable.

ios-doctor:
	@bash scripts/ios-doctor.sh

ios-setup:
	@bash scripts/ios-doctor.sh --fix

ios-init:
	pnpm run ios:init

ios-simulator:
	@bash scripts/ios-simulator.sh

ios-dev:
	@bash scripts/ios-dev.sh

ios-build:
	pnpm run ios:build

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
