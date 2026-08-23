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
above it. `scripts/gen-changelog-draft.sh` generates a draft of the Unreleased
section from git history — edit it by hand before tagging.
-->

## [Unreleased]

### Added

- **Live versioning** — every build now reports its true version, derived from
  git tags at build time. Local development builds report `development` instead of
  a misleading fixed number.
- **Embedded changelog** — this changelog is bundled into every target and the UI
  shows a "What's New" dialog the first time it runs after an upgrade.
- **GitHub Releases pipeline** — tagging `vX.Y.Z` builds all targets and publishes
  a GitHub Release whose notes are taken from this file (see [RELEASING.md](RELEASING.md)).

## [0.1.0] - 2026-08-23

### Added

- Initial slicer engine: STL/OBJ/3MF loading, mesh slicing, Arachne
  variable-width wall generation, top/bottom surface detection, and infill.
- Unified scene engine (single source of truth for object placement) shared by
  the CLI, WebSocket server, and WASM UI.
- Angular UI with a Three.js viewport and G-code preview, plus a Tauri desktop
  shell.
- Command-line interface with `slice`, `info`, and schema-generation commands.
