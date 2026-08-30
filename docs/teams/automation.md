# Automation and the CLI

The same engine that runs behind the interface is a single binary you can call
from a script. No UI, no server, no browser.

```bash
slicer-engine slice --input model.stl --output model.gcode
```

## A whole plate in one command

Repeat `--input` for a multi-object plate. `--arrange` packs it without overlap.

```bash
slicer-engine slice \
  -i part_a.stl -i part_b.stl -i part_c.stl \
  --arrange --arrange-spacing 3 \
  --output plate.gcode
```

Transform flags apply to the whole plate, since the CLI has no syntax for
addressing one object: `--translate`, `--rotate`, `--scale`, `--align-face`,
`--center`, `--drop-to-floor`.

Objects that fall outside the build volume or overlap each other are reported as
warnings. The slice still runs.

## Machine-readable output

```bash
slicer-engine slice --input model.stl --output-format json
```

That's what makes the CLI worth scripting: parse the result for layer count,
estimated time and material, and gate a pipeline on it.

`settings`, `config` and `info` all take `--output-format json` too.

## Useful flags

| Flag | Does |
| --- | --- |
| `--layer-height` | Override layer height for this run |
| `--gcode-flavor` | `marlin` or `klipper` |
| `--start-print-gcode` / `--end-print-gcode` | A string, or a path to a file |
| `--config` | Use an explicit project config instead of auto-discovery |
| `--arrange` | Pack every input onto the bed |
| `--arrange-auto-orient` | Orient each part while arranging |
| `--verbose` | Mesh statistics and per-phase timing |
| `--output-format` | `human` or `json` |

`slicer-engine slice --help` is authoritative.

## Recipes

**Slice a folder.**

```bash
for f in models/*.stl; do
  slicer-engine slice --input "$f" --output "gcode/$(basename "${f%.stl}").gcode"
done
```

**Fail a build when a part won't print.** Slice in CI with a committed
`slicer.toml`; a non-zero exit means the model or the settings are wrong, and
you find out on push rather than on the printer.

```yaml
- name: Slice reference parts
  run: |
    for f in parts/*.stl; do
      slicer-engine slice --input "$f" --output /tmp/out.gcode --output-format json
    done
```

**Quote a job.** Slice with `--output-format json` and read the estimates
straight out of the result.

**Pin a customer's job.** Keep `slicer.toml` beside their models. Re-slicing a
year later gives the same G-code, because config and engine version both pin it.

## Reproducibility

Two things determine the output: the resolved configuration and the engine
version. Pin both — commit the `slicer.toml`, record the version from
`slicer-engine info` — and a re-slice is a re-slice, not a re-negotiation.

The server's G-code cache follows the same rule: an identical scene with an
identical configuration returns the cached file instead of re-slicing, and any
engine upgrade invalidates it automatically.

## Where the CLI can't go

`slice`, `settings`, `config`, `info` and `changelog` all work headlessly.
`serve` needs a built interface directory. iOS builds ship without the CLI at
all — a sandboxed app has no command line.

## Reference

- [CLI reference](/architecture/cli) — every command and flag
- [Configuration](/teams/configuration) — the layered `slicer.toml`
- [G-code](/architecture/gcode) — dialects, markers, what comes out
