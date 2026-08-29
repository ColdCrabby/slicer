# Test fixtures

Small, checked-in inputs used by the Rust test suite. Keep them minimal — they
are read on every test run and live in the repository forever.

| Fixture                                          | Used for                                                                                     | Origin                       |
| ------------------------------------------------ | -------------------------------------------------------------------------------------------- | ---------------------------- |
| `simple-cube.stl` / `.obj` / `.3mf` / `-ascii.stl` | Loader round-trips across the supported mesh formats                                          | Generated                    |
| `simple-cube.gcode`                              | G-code parsing and preview tests                                                              | Generated                    |
| `global.json`, `object.json`                     | Settings / profile schema tests                                                               | Generated                    |
| `TopAC.3mf`                                      | **Multi-object 3MF** — two named build items (`top`, `bottom`) at different heights           | Contributed by **@max-scopp** |

## About `TopAC.3mf`

A real-world model, contributed by **@max-scopp** and used here with their
permission. It is the regression fixture for multi-object 3MF support: the file
declares two `<object>` resources with `name` attributes and places both via
`<build><item>`.

It is worth keeping precisely *because* it is real. It catches three things a
hand-written fixture tends to miss:

- the two parts must stay **separate** scene objects rather than fusing into one
  mesh (`read_3mf_objects_from_bytes`),
- their authored **names** must survive to the object list, since that is the
  only human-readable label a 3MF carries, and
- the parts sit at **different Z spans**, so a loader that silently returned the
  whole merged model for each part would be caught by the differing geometry
  rather than passing unnoticed.

See [src/mesh/io.rs](../../src/mesh/io.rs) and
[src/scene/ops.rs](../../src/scene/ops.rs) for the tests that consume it.
