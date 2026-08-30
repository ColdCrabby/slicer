# Configuration

The interface saves settings into profiles. The CLI and the server read a
**`slicer.toml`**. This page is about the latter — how to pin defaults across a
team, and how to make one job slice identically every time.

## The layers

Four layers, deep-merged, each overriding the last:

```
built-in defaults
  → user config      ~/.config/slicer-engine/slicer.toml
    → project config ./slicer.toml
      → CLI flags
```

Deep-merged means a project file only writes what it changes. Everything else
falls through from the layer below. Command-line flags override for that one
invocation and are never written back.

## Creating one

```bash
slicer-engine config init          # writes ./slicer.toml
slicer-engine config path          # where the global one lives
slicer-engine config show          # the fully merged result
slicer-engine config show --output-format json
```

`config show` is the one to reach for when a setting isn't doing what you
expect — it tells you what the slicer actually resolved, not what any single
file says.

## What goes in it

```toml
[machine]
name = "Voron 2.4"
nozzle_diameter = 0.4
build_volume_x = 350.0
build_volume_y = 350.0
build_volume_z = 340.0

[slicing]
layer_height = 0.2
wall_count = 3
infill_density = 0.20

[server]
host = "0.0.0.0"
port = 5201
work_dir = "/var/lib/coldcrabby"
```

| Section | Covers |
| --- | --- |
| `[machine]` | Nozzle, build volume, speed and acceleration limits |
| `[slicing]` | Every slicing parameter — the same set the Process tab shows |
| `[server]` | Bind address, port, interface directory, work directory, CORS origins |
| `[profiles]` | Named presets, machines and materials |
| Top level | `start_print_gcode`, `end_print_gcode`, lifecycle markers |

Full field reference: [Config](/architecture/config) and
[Settings](/architecture/settings).

## Reading and writing single values

```bash
slicer-engine settings show
slicer-engine settings get layer_height
slicer-engine settings set layer_height 0.15
slicer-engine settings set gcode_flavor klipper
slicer-engine settings set start_print_gcode null   # clear an optional field
```

Both flat aliases (`layer_height`) and full paths (`params.layer_height`) work.
These write the **global** config.

## Patterns worth stealing

**Per-job reproducibility.** Commit a `slicer.toml` next to the job's models.
Anyone who slices in that directory — or any pipeline that does — gets identical
output, whatever their personal defaults are.

```
jobs/bracket-v3/
├── slicer.toml
├── bracket.stl
└── plate.stl
```

**A team baseline.** Ship one `slicer.toml` to every workstation's user config
directory. People can still override per project, and nobody starts from
factory defaults.

**Machine-specific overrides.** Keep the shared baseline in the user config and
put only the differences — nozzle, build volume, start G-code — in a per-machine
project file.

## Validating before you print

```bash
slicer-engine settings validate --global global.json --object object.json
slicer-engine settings diff     --global global.json --object object.json
```

`validate` checks values against physical constraints. `diff` shows what an
object-level file actually overrides — useful when a per-part override is doing
something you didn't intend.

## Profiles vs. `slicer.toml`

Both configure the slicer; they aren't the same thing.

| | Profiles (`profiles.toml`) | Config (`slicer.toml`) |
| --- | --- | --- |
| Edited from | The interface | A text editor, or `config` / `settings` commands |
| Shape | A library of named printers, filaments, processes | One resolved configuration |
| Best for | What people pick between | Defaults and per-job pinning |

A team usually wants both: a shared profile library so everyone picks from the
same list, and a project `slicer.toml` wherever a job must be reproducible.

::: details Advanced — legacy JSON
Older `settings.json` and `slicer.json` files still load, for migration. Current
code only ever writes TOML.
:::
