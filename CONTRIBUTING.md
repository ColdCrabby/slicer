# Contributing

Thanks for wanting to help. This page is the workflow; the rest of the docs
carry the detail.

## Get set up

Prerequisites and first-run steps live in [SETUP.md](SETUP.md). The short
version:

```bash
git clone https://github.com/YOUR-USERNAME/slicer-engine.git
cd slicer-engine
pnpm install          # workspace deps + git hooks
cargo build && cargo test
```

`pnpm install` installs [Lefthook](https://lefthook.dev), which formats your
**staged** files on every commit — `rustfmt` for Rust, Prettier for the Angular
UI. Formatting will never block your PR. Skip once with `--no-verify`, disable
with `LEFTHOOK=0`.

## Find your way around

- [ARCHITECTURE.md](ARCHITECTURE.md) — the map. Which module owns what.
- Module READMEs — the real documentation. Start with
  [`src/core/README.md`](src/core/README.md).
- [AGENTS.md](AGENTS.md) — the contracts. Long, but it's where the
  non-obvious invariants are written down, and most review comments trace back
  to something in it.
- [DEVELOPMENT.md](DEVELOPMENT.md) — day-to-day commands.

Reading `process_mesh` in [`src/core/pipeline.rs`](src/core/pipeline.rs) is the
fastest way to understand what actually happens.

## Make the change

Branch names describe the change: `feature/gyroid-infill`,
`fix/arachne-thin-wall-crash`, `docs/explain-surface-detection`.

**Rust:** standard idioms, `///` on public APIs, tests inline in `#[cfg(test)]`
modules beside the code they cover.

**UI:** see [the component structure guide](.github/instructions/angular-component-structure.instructions.md)
and [the design language](.github/instructions/ui-design-language.instructions.md).

**Commits:** `type(scope): short description`, wrapping the body at 72
characters. Types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`. Explain
the motivation, not just the mechanism.

## Before you open the PR

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

CI runs these on every platform target. Don't bypass it.

If your change touches geometry — anything in `core/`, `walls/`, `infill/`,
`adhesion/` or `gcode/` — **measure it**.
[`tools/gcode-analysis/`](tools/gcode-analysis/README.md) has scripts for wall
overlap, unfilled gaps, bead-width distribution and capsule renders. Compare
against the `classic` generator, which is the trusted reference, and attach
before/after images to the PR. A plausible explanation is not evidence; the QA
gate has caught changes that looked obviously correct and destroyed real infill.

Not sure what to check? Ask the agent "what should I test?" — the
[`test-changes` skill](.github/skills/test-changes/SKILL.md) answers with a
checklist for the platform you name.

## Pitfalls that bite everyone once

**Clipper2 fill rules.** `EvenOdd`, `Positive` and `NonZero` are not
interchangeable, and the wrong one produces geometry that looks fine until it
doesn't. The table in [`src/core/README.md`](src/core/README.md) says which goes
where.

**Winding order.** Solid contours are CCW, holes CW. Normalising everything to
CCW makes holes fill with plastic.

**Parallel arrays.** `path_roles`, `path_widths`, `path_objects` and friends are
indexed alongside `paths`. Anything that rebuilds a layer's paths must carry
them all, or roles and widths shift onto the wrong paths silently.

**New settings fields** need `#[serde(default)]` and a default function, or old
config files stop loading.

**Don't add a second source of truth.** Placement belongs to `scene/`, version
belongs to `src/version.rs`, config belongs to `config/`. Adding a parallel one
is the failure mode these modules exist to prevent.

## Documentation

Update docs in the same PR as the change.

- **Module READMEs** are the deep documentation, written in the
  [Diátaxis](https://diataxis.fr/) *Explanation* style — what something is and
  *why* it's that way. [`src/scene/README.md`](src/scene/README.md) is the model
  to follow; the house style is described in [AGENTS.md](AGENTS.md).
- **[ARCHITECTURE.md](ARCHITECTURE.md)** is a map. Keep it a map.
- **User-facing docs** live in [`docs/use/`](docs/use/) and
  [`docs/teams/`](docs/teams/). If your change alters what a user sees or does,
  it belongs there too.
- **[CHANGELOG.md](CHANGELOG.md)** — add to `## [Unreleased]` for anything a
  user would notice.

Don't put issue or PR numbers in prose. Describe the change instead; the number
means nothing to someone reading the changelog in two years.

## What to work on

Issues labelled `good first issue` and `help wanted` are the obvious starting
points. Documentation, tests for existing code, and small well-reproduced bugs
are always welcome.

If you're planning something large, open a discussion first — the pipeline has
ordering constraints that aren't obvious from the outside, and it's easier to
point them out before you've written the code.

## Getting help

[Discussions](https://github.com/max-scopp/slicer-engine/discussions) for
questions, [Issues](https://github.com/max-scopp/slicer-engine/issues) for bugs.

## Code of conduct

Be respectful, constructive and inclusive. Welcome newcomers, give feedback on
the code rather than the person, and assume good intent. Harassment,
discriminatory language and personal attacks aren't tolerated.

Report problems by opening an issue or contacting the maintainer directly.
