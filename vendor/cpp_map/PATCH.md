# `cpp_map` — vendored with one local patch

Upstream: <https://codeberg.org/eadf/cpp_map_rs>, version 0.2.0 (commit
`1912fd11818b599948ecafdfa6f9af3bc56eec7e`), MIT OR Apache-2.0. Vendored **only**
to carry the patch below; nothing else is changed, so it can be dropped the
moment the fix lands upstream. The registry's own metadata (`.cargo-ok`,
`.cargo_vcs_info.json`, `.cargo/`) and the upstream CI config were left out.

## Why

`cpp_map`'s skip list drew its node levels from `rand::rng()` — an OS-seeded
generator — through a thread-local `ThreadRng`. For a map that is only queried,
that is harmless: levels decide the list's *balance*, never its contents.

`boostvoronoi` does not only query it. It uses the skip list as the **beach
line** of its sweep, and walks it. The list's random shape therefore reaches the
Voronoi diagram, which reaches the medial axis, which reaches the walls we print.

The effect, measured on one 97-segment region taken from a 3DBenchy slice: twenty
builds of the *same* input produced **five different diagrams** — 336 to 344
vertices, 1058 to 1074 edges — while the cell count stayed correct at 194. That
made the whole slicer non-deterministic: the same model, sliced twice, produced
different G-code. Only the Arachne generator was affected; Classic never builds a
Voronoi diagram.

## The patch

Two changes, both in `src/skiplist/skiplist_impl.rs` and both marked
`LOCAL PATCH`:

1. The thread-local generator is a `StdRng` seeded from a constant, not a
   `ThreadRng` seeded from the OS.
2. `SkipList::with_params` re-seeds it for every new list. Seeding once per
   thread is not enough — the generator keeps advancing, so the second list built
   on a thread would draw a different stream than the first and two identical
   Voronoi builds would still disagree. This is the change that actually fixes
   it; the first alone does not.

`rand::prelude::ThreadRng` is spelled out in several signatures, so those
mentions are rewritten to `rand::rngs::StdRng` as a consequence of (1).

Levels stay geometrically distributed with the same `p`, so the list keeps its
expected O(log n) balance — it is now the *same* balance every run.

## Verifying

`tests/determinism.rs` in the engine slices a 3DBenchy twice with Arachne and
asserts byte-identical G-code. It fails without this patch.
