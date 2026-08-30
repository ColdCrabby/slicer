# Known-bad mesh corpus

One 10 mm cube per defect class, used by
[../../mesh_repair.rs](../../mesh_repair.rs) to pin
[`mesh::repair`](../../../src/mesh/repair.rs). Each file isolates a single
problem, so a failing test names the repair step that broke rather than "the
repair pass regressed".

| File                       | Defect                                                              |
| -------------------------- | ------------------------------------------------------------------- |
| `cube-hole.stl`            | Top face missing → one 4-edge boundary loop                          |
| `cube-flipped-face.stl`    | One triangle wound against its neighbours                            |
| `cube-inverted.stl`        | Every triangle wound inward — watertight but inside out              |
| `cube-degenerate.stl`      | Two zero-area triangles (collinear points, and a repeated corner)    |
| `cube-duplicate-faces.stl` | One triangle written twice                                           |
| `cube-unwelded.stl`        | A crack: the bottom face uses a corner nudged 1e-5 mm off its neighbours' |
| `cube-multi-defect.stl`    | Hole + flipped triangle + duplicate + degenerate, all at once        |

Every one of them must come out of the repair pass watertight, manifold,
outward-facing and shaped like a ~1000 mm³ cube.

## Regenerating

```bash
python3 tests/fixtures/broken/generate.py
```

The generator is deterministic — re-running it must not produce a diff. Add new
defect classes there rather than committing hand-edited binaries, so the corpus
stays reproducible.

## Trying them by hand

```bash
cargo run -- mesh-check --input tests/fixtures/broken/cube-multi-defect.stl
cargo run -- mesh-check --input tests/fixtures/broken/cube-hole.stl --no-mesh-repair
```

## See also

- [src/mesh/README.md](../../../src/mesh/README.md#validation-and-repair) — why
  the repair pass exists and what it does
- [issue #114](https://github.com/ColdCrabby/slicer/issues/114)
