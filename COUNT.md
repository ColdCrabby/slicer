# Codebase Size

Last updated: 2026-09-29

Counted with `cloc` 1.98, excluding build artifacts, lock files, generated code, vendored code and 3D-model fixtures.

## Total

| | SLOC | Files |
|---|---|---|
| Everything (incl. Markdown docs) | 140,871 | 833 |
| Code only (excl. Markdown prose) | 127,555 | ~762 |

Tests counted separately: 2,419 SLOC / 24 files (`tests/`). Without tests: 138,452 SLOC / 809 files.

## By language

| Language | SLOC | Files |
|---|---|---|
| Rust | 59,138 | 163 |
| TypeScript | 40,779 | 329 |
| Markdown | 13,316 | 71 |
| SCSS | 11,320 | 95 |
| HTML | 10,817 | 78 |
| Python | 1,226 | 14 |
| YAML | 1,101 | 14 |
| JSON | 982 | 32 |
| Bourne Shell | 961 | 12 |
| JavaScript | 631 | 9 |
| TOML | 209 | 6 |
| Vue Component / XML / Make / CSS / misc | 391 | 11 |

## By top-level directory

| Directory | SLOC | Dominant content |
|---|---|---|
| `src/` (Rust engine core) | 60,258 | Rust 55,029 (+ 4,917 md/`SLICING.md` docs) |
| `ui/` (Angular frontend) | 63,127 | TR 40,408, SCSS 10,396, HTML 10,386 |
| `docs/` (website) | 3,899 | Markdown 2,325, SCSS 924 |
| `ui-desktop/` (Tauri shell) | 2,852 | Rust 1,942 |
| `tests/` | 2,419 | Rust integration tests 2,087 |
| `scripts/` (build/release tooling) | 1,596 | Shell + JS |
| `tools/` | 1,249 | Python 1,038 |
| `analysis/` | 105 | Python probes |
| Root (Cargo.toml, build.rs, Makefile, CI YAML…) | ~4,966 | — |
