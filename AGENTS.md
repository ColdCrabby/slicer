# Slicer Engine — AI Agent Guidance

High-performance 3D model slicer engine written in Rust.

This file is a **map, not a reference**. Read the relevant module `README.md` before changing a subsystem. Detailed implementation knowledge belongs beside the code.

## Development

- Use the seeded dev launcher (`pnpm run dev`); **never hardcode dev ports**.
- Validate changes with the relevant tests.
- Lefthook handles Prettier and `rustfmt` automatically on commit.
- UI-only work does not need Rust: download the `ui-hydrated` CI artifact instead of running `pnpm run hydrate` (see `DEVELOPMENT.md`).
- A tripped bundle budget is explained in the Frontend CI job summary; read it before tracing imports by hand (see `ui/README.md`).
- Do not bypass CI checks.

## Architecture

- Follow existing architecture and single sources of truth. Do not introduce parallel implementations or shortcuts.
- Placement goes through the scene engine; do not directly transform meshes.
- `slice_plate` is the single slicing entry point.
- New pipeline steps are stages in `src/core/stages.rs`, and optional features plug in through `src/plugin/` — never by editing `pipeline.rs`.
- Slice requests reference profiles by ID; profiles remain engine-side.
- Printer traffic goes through the slicer, never directly from the browser.
- Preserve WASM/iOS platform boundaries.

## Validation

- For slicing or geometry changes, validate against the existing implementation.
- Geometry changes require before/after visual verification in the PR.
- Do not claim a fix without measuring or testing the affected behavior.

## Documentation

- User-visible changes must update `docs/use/`.
- Module-specific knowledge belongs in the module `README.md`.
- Detailed implementation rationale belongs in `///` documentation next to the relevant code.
- Avoid documenting one-off debugging incidents; document the resulting rule instead.

## PRs

Before opening a PR:

- Ensure relevant tests pass.
- Update affected documentation.
- **Update `CHANGELOG.md` for every user-visible change.**
- Include before/after evidence for geometry changes.
- Clearly describe what changed and why.
- Ensure CI passes before merge.

## Important References

- `ARCHITECTURE.md` — high-level design
- `DEVELOPMENT.md` — development workflow
- `CONTRIBUTING.md` — contribution process
- `RELEASING.md` — versioning and releases
- `CHANGELOG.md` — user-facing release notes
- Module `README.md` files — subsystem-specific rules and architecture
