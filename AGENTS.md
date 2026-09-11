# Slicer Engine — AI Agent Guidance

A high-performance 3D model slicer engine written in Rust, powered by
[Clipper2](https://github.com/AngusJohnson/Clipper2) for polygon clipping.

**This file is a map, not a reference.** Depth lives in the module README it
points at — that is the house rule for
[ARCHITECTURE.md](ARCHITECTURE.md) and it applies here too. If you are about to
add thirty lines explaining how a subsystem works, they belong in that
subsystem's README, and a one-line pointer belongs here.

---

## Quick commands

```bash
cargo build                  # fast iteration (debug, opt-level 1) — ~5-10s
cargo test
cargo fmt && cargo clippy --all-targets --all-features -- -D warnings
pnpm run dev                 # engine + UI on a seeded port pair
```

Cross-platform builds go through the Makefile: `make build-release build-windows
build-macos build-wasm test lint fmt`. Release builds use LTO and are reserved
for CI and distribution — do not wait on one during iteration.

### Dev servers run on a random seed — never a fixed port

`pnpm run dev` ([scripts/dev.mjs](scripts/dev.mjs)) rolls a three-digit **seed**
(200–999) and derives everything from it: the UI on `4<seed>`, the engine on
`5<seed>`, and a work directory of its own. It verifies the ports are free
first, so a second worktree, a colleague on the same box, or a parallel agent
session never collides with yours. `--seed` pins one, `--print` resolves ports
without starting anything, and `dev:web-slicer` / `dev:desktop` cover the other
runtimes.

**The engine's port is an internal detail.** The dev server proxies `/api` and
`/ws` to it ([ui/proxy.conf.mjs](ui/proxy.conf.mjs)), so the browser addresses a
single origin in development exactly as in production — which is why
[environment.ts](ui/src/environments/environment.ts) carries no port at all.
Never reintroduce one there: it would pin the UI to one instance of the engine
and break every seeded run but the first. **Report the URL the launcher prints**,
never a hardcoded `:4213`.

---

## Where things live

Every module owns its own explanation. Start at the README, not the source.

| Module | Owns | README |
| --- | --- | --- |
| `src/mesh/` | Mesh types, loaders, analysis, import-time repair | [README](src/mesh/README.md) |
| `src/scene/` | **SSOT for object placement** — `SceneOp`, `SceneState` | [README](src/scene/README.md) |
| `src/core/` | The slicing pipeline, surfaces, infill boundary, supports, object identity | [README](src/core/README.md) |
| `src/walls/` | Perimeter generation (Arachne + classic) | [README](src/walls/README.md) |
| `src/infill/` | Pattern generation inside a boundary | [README](src/infill/README.md) |
| `src/adhesion/` | Skirt, brim, raft | [README](src/adhesion/README.md) |
| `src/orient/` | Auto-orientation | [README](src/orient/README.md) |
| `src/gcode/` | `Vec<SliceLayer>` → firmware-ready G-code | [README](src/gcode/README.md) |
| `src/settings/` | `SlicingParams`, validation, the JSON Schema | [README](src/settings/README.md) |
| `src/profiles/` | The user's printers, filaments, processes; export | [README](src/profiles/README.md) |
| `src/printer/` | Outbound transport to real printers (native only) | [README](src/printer/README.md) |
| `src/server/` | HTTP + WebSocket host, the G-code result cache | [README](src/server/README.md) |
| `src/config/` | `slicer.toml`, `config_dir()` | [README](src/config/README.md) |
| `src/db/` | SQLite history + migrations | [README](src/db/README.md) |
| `src/cli/` | Command catalog and argument reference | [README](src/cli/README.md) |
| `ui/` | The Angular front-end | [README](ui/README.md) |
| `ui-desktop/` | One Tauri shell for desktop **and** iPadOS/iOS | [README](ui-desktop/README.md) |

[ARCHITECTURE.md](ARCHITECTURE.md) is the high-level design map;
[DEVELOPMENT.md](DEVELOPMENT.md) has the dev workflow and common tasks.

---

## Contracts you must not break

Each of these is a single-source-of-truth rule that has already been violated at
least once, with real consequences. The linked README explains why.

- **Placement goes through the scene engine.** Every CLI flag and every UI
  gesture becomes a `SceneOp`; transforms are baked once, at the slicer
  boundary. Never "just translate this mesh real quick".
  → [scene](src/scene/README.md)
- **Every slice entry point resolves objects *and* parts.** All four runtimes
  load with `load_*_multi` and pick `parts[source_part]`. A merging loader in a
  slice path silently prints the file instead of the plate.
  → [scene](src/scene/README.md#every-slice-entry-point-resolves-objects-and-parts--all-four-of-them)
- **`slice_plate` is the only slicing entry point**, and its merged fast path
  must stay byte-identical when nothing asks for object awareness.
  → [core](src/core/README.md#object-identity-through-slicing)
- **A git tag is the single source of truth for a release.** The version is
  derived at build time by [build.rs](build.rs) via `git describe`, and
  [src/version.rs](src/version.rs) is the one place every target reads version
  and changelog from. **Never add a parallel version constant** — especially not
  in the UI. `Cargo.toml`'s `version` is the *next* target version only.
  → [RELEASING.md](RELEASING.md)
- **The UI renders the changelog from exactly one component**, so the What's New
  settings section and the post-upgrade dialog can never drift apart.
  → [RELEASING.md](RELEASING.md)
- **Profiles persist where the engine runs**, not only in `localStorage`, or a
  cloud user who clears their browser loses every printer they own.
  → [profiles](src/profiles/README.md)
- **Printer traffic goes slicer → printer, never browser → printer**, and the
  transport is chosen at *runtime*, not from a build-time constant.
  → [printer](src/printer/README.md)
- **`cli`, `server` and `db` are excluded from iOS**, in `cfg`s and in the
  dependency tables together. → [ui-desktop](ui-desktop/README.md)
- **Nothing routed in the UI uses a static `component:`**, and a root-provided
  service drags its whole import graph into the initial bundle.
  → [ui](ui/README.md#what-may-sit-in-the-initial-download)
- **Phones, tablets and touchscreens are three separate questions**, asked
  independently — never one `max-width`. → [ui](ui/README.md#phones-and-tablets)
- **Mesh repair is deterministic and borrows clean meshes.** A clean mesh is
  returned `Cow::Borrowed` and never rebuilt, which is what makes default-on
  repair safe for the QA baselines. → [mesh](src/mesh/README.md)
- **The `gcode_cache` table is written on every slice and never read to skip
  one.** `Db::get_cached_gcode` still exists and is still tested, but wiring it
  back into `handle_slice` would let a "cached" response serve stale G-code for
  a scene the engine has since changed how it slices. Discuss before changing.
  → [server](src/server/README.md#the-g-code-result-table--written-on-every-slice-never-read-to-skip-one)
- **Support generation reads a pristine perimeter snapshot**, not the live
  layer — `classify_overhang_perimeters` has already split the very walls it
  would measure. Any test for support behaviour must go through `process_mesh`.
  → [core](src/core/README.md#support-structure-generation)

---

## Conventions

### Code style

- [Rust Edition 2021](https://doc.rust-lang.org/edition-guide/rust-2021/index.html);
  `cargo fmt` and `cargo clippy -- -D warnings` are enforced by CI.
- Inline tests with `#[cfg(test)]` in the same module.
- `///` doc comments on public types and functions, with usage examples for core
  APIs. **Reference material belongs in doc comments, not in a module README.**

### Validate, don't guess

[tools/gcode-analysis/](tools/gcode-analysis/README.md) measures sliced G-code
directly — wall overlap, unfilled wall-zone gaps, length-weighted bead widths,
and capsule/gap renders. **Compare a change against the `classic` generator (the
trusted reference) before claiming a fix.** If geometry changed, the PR needs a
before/after picture — see
[`slicing-visual-verification`](.github/instructions/slicing-visual-verification.instructions.md).

### Performance

Debug builds locally, release in CI. Minimise allocations in slicing hot paths,
and profile with `cargo flamegraph` rather than guessing when a regression is
suspected. Be mindful of WebAssembly memory limits on large models.

### Documentation

Docs are split by audience, and a change usually belongs in exactly one place.

| Audience | Lives in | Style |
| --- | --- | --- |
| **Users** — how to use the app | [docs/use/](docs/use/) | Plain language, task-first. Simple by default; advanced detail kept but terse, in `::: details` blocks. |
| **Teams** — deploying and operating it | [docs/teams/](docs/teams/) | Self-hosting, shared config, automation, data and licensing. Assumes an administrator. |
| **Brand** | [docs/brand.md](docs/brand.md) | Name, mascot, assets, palette, voice. |
| **Contributors** — how it works | Module `README.md`s, [ARCHITECTURE.md](ARCHITECTURE.md) | Explanation, not reference. See below. |

**A user-visible change is not done until [docs/use/](docs/use/) reflects it** —
a new setting, a new button, a changed shortcut, all of it. Update
[README.md](README.md) for headline features only.

#### Module READMEs — house style

Long-form module docs follow the [Diátaxis](https://diataxis.fr/) **Explanation**
quadrant: what something is and *why* it is that way, not how to call every
function. [src/scene/README.md](src/scene/README.md) is the canonical example.

- **Open with a one-sentence answer to "what does this module exist for?"**,
  followed by the single rule the rest of the doc defends.
- **Lead with motivation, then contract, then anatomy.** Why → rules → shapes →
  catalog → role in the wider system → lifecycle → non-goals.
- **Small Mermaid diagrams** where a picture saves a paragraph. Several focused
  ones beat a monster graph. Keep node labels short.
- **Compact tables for catalogs** — three or four columns, one-line cells.
- **State the non-goals explicitly.** A "what this module deliberately does
  *not* do" section prevents drift back into anti-patterns.
- **Plain language over jargon.** Assume a contributor who knows Rust but is new
  to *this* subsystem. Define a term the first time it appears.
- **End with a "See also"** pointing at the source files and related modules.

**Detail belongs beside the code, not in the README.** A threshold's exact value,
why it is not one notch lower, and what was tried before it — all of that goes in
the `///` comment on the constant or function, where someone about to change it
will actually read it. The README says the correction exists, what it is keyed
to, and what breaks if you generalise it; then it points at the code.

**Do not write the incident up.** A measured number from a debugging session, a
defect's nickname, or a narrative of what went wrong once is not documentation —
nobody holds it in mind, and it buries the rule it came from. Keep the rule and
one clause of why ("never X — it erases thin features"); drop the forensics. The
exception is a rule that looks arbitrary enough to be "simplified" away, and one
sentence of consequence is enough to protect it.

#### The docs site wears the app's design language

[docs/.vitepress/theme/styles/_tokens.scss](docs/.vitepress/theme/styles/_tokens.scss)
`@use`s the **real** theme partials from the shared UI library
(`ColdCrabby/ui`, vendored into `ui/vendor/coldcrabby-ui` by `ui`'s postinstall)
through the Sass `loadPaths` in [docs/.vitepress/config.ts](docs/.vitepress/config.ts) —
the same idiom `ui/angular.json` uses — and maps them onto VitePress's `--vp-*`
variables. **Never write a colour,
radius, duration or font literal into the docs theme** — add a mapping in
`_tokens.scss` instead, so changing the accent in the library recolours the docs
and the two cannot drift.

Two things to know before editing it:

- **The docs build depends on the vendored UI checkout.** A `docs:build` in a
  tree that never ran `ui`'s postinstall fails with "Can't find stylesheet to
  import" — run `pnpm --filter slicer-ui vendor:ui` first.
- **VitePress's component styles are scoped**, so `.VPButton.medium[data-v-…]`
  outranks a plain `.VPButton.medium`. Doubling the class is how the theme wins
  that without reaching for `!important`.

Where the docs need a live piece of the design language rather than a
description of it, use a Vue component under `theme/components/` — `Swatches`
prints each chip's *resolved* colour, so the brand page cannot quote a hex the
product no longer uses.

The docs site carries a temporary **"early docs" banner** while the structure
settles: a `layout-top` slot in `docs/.vitepress/theme/`. To remove it, delete
`Banner.vue`, `banner.css` and the `theme/` registration in `index.ts` —
including `--vp-layout-top-height` in `banner.css`, which is what reserves its
space, or the nav keeps a gap above it.

---

## Known constraints and pitfalls

- **Clipper2 coordinates are integers** (`Centi` precision). Mind the conversion
  from floating-point models. Which fill rule to use where is in
  [core](src/core/README.md#which-clipper2-fill-rule-and-why).
- **CLI framework is clap v4** — keep derive macros in sync with the command.
- **File I/O in WASM** requires JavaScript bindings; not every CLI feature
  exists there.
- **Cross-compilation needs the target toolchains installed.** CI verifies them.
- **LTO makes release builds slow.** Use debug builds while iterating.
- **`sudo xcode-select -s /Applications/Xcode.app` is unavoidable** for iOS.
  `DEVELOPER_DIR` fixes the helper scripts but **`tauri ios dev` builds with a
  sanitized environment and never forwards it**, so do not "fix" this with an
  export — it silently does nothing for the actual build.

---

## CI/CD

[.github/workflows/build.yml](.github/workflows/build.yml) runs on push and pull
request: builds all platform targets, runs clippy and fmt checks, and executes
the test suite. **Do not bypass CI checks.** All builds must pass before merge.

Releases are tag-driven —
[.github/workflows/release.yml](.github/workflows/release.yml) fires on `v*`
tags. See [RELEASING.md](RELEASING.md).

---

## Task-specific instructions

`.github/instructions/*.instructions.md` hold the rules for particular kinds of
work. They are shared with GitHub Copilot (which applies them automatically via
`applyTo`) and reached by Claude through the skills in `.claude/skills/`. Each
file's frontmatter says when it applies — **read the matching one instead of
improvising the same rule**:

| Instruction | Applies to |
| --- | --- |
| [`ui-design-language`](.github/instructions/ui-design-language.instructions.md) | Any styling, theming or visual-polish work in `ui/` |
| [`progressive-disclosure`](.github/instructions/progressive-disclosure.instructions.md) | Exposing a setting or capability in the UI |
| [`angular-component-structure`](.github/instructions/angular-component-structure.instructions.md) | Creating, splitting or reviewing Angular components |
| [`ui-style-no-build`](.github/instructions/ui-style-no-build.instructions.md) | Build-verification policy for UI work |
| [`slicing-visual-verification`](.github/instructions/slicing-visual-verification.instructions.md) | Any change that alters sliced geometry |
| [`no-issue-numbers`](.github/instructions/no-issue-numbers.instructions.md) | Any prose a human reads |
| [`no-blocking-waits`](.github/instructions/no-blocking-waits.instructions.md) | Anything that could involve waiting on CI |

Specialized agents live in `.claude/agents/*.md` (mirrored for Copilot at
`.github/agents/*.agent.md`) and skills in `.claude/skills/*/SKILL.md`. Claude
discovers both automatically.

---

## Related documentation

- [README.md](README.md) — user guide and feature overview
- [SETUP.md](SETUP.md) — prerequisites and first run
- [DEVELOPMENT.md](DEVELOPMENT.md) — dev workflow and common tasks
- [CONTRIBUTING.md](CONTRIBUTING.md) — contribution process
- [ARCHITECTURE.md](ARCHITECTURE.md) — high-level design map
- [RELEASING.md](RELEASING.md) — versioning, changelog, release process
- [CHANGELOG.md](CHANGELOG.md) — embedded, user-facing release notes
- [docs/use/](docs/use/) · [docs/teams/](docs/teams/) — end-user and operator docs
- [tools/gcode-analysis/](tools/gcode-analysis/README.md) — G-code quality diagnostics
