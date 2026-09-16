# gcode analysis toolkit

Diagnostic scripts for inspecting sliced G-code quality — built while fixing the
Arachne wall generator and its variable-width gap fill. They measure and
visualise wall/gap-fill defects straight from a `.gcode` file, and are meant to
be run against output from `slicer-engine slice`.

The guiding workflow is **slice → measure → compare against the `classic`
generator** (the trusted reference) rather than eyeballing a viewer.

## Requirements

- `python3` with `numpy` (all scripts) and `matplotlib` (`render.py`, `zoom.py`).

```bash
pip install numpy matplotlib
```

## Producing input

```bash
cargo build
printf '[slicing]\nwall_generator = "arachne"\n' > /tmp/arachne.toml
printf '[slicing]\nwall_generator = "classic"\n' > /tmp/classic.toml
./target/debug/slicer-engine slice -i 3DBenchy.stl --config /tmp/arachne.toml -o /tmp/arachne.gcode
./target/debug/slicer-engine slice -i 3DBenchy.stl --config /tmp/classic.toml -o /tmp/classic.gcode
```

## Scripts

| Script | What it measures | Usage |
| --- | --- | --- |
| `coincident.py` | Overlapping wall beads: near-parallel, non-adjacent segments closer than a gap threshold. Target **0** for clean walls. | `coincident.py <gcode> [layer=60] [gap_mm=0.10]` |
| `voids.py` | Enclosed **wall-zone gaps** — thin (`< 2.5×nozzle`) unfilled voids hugging walls/gap-fill but not infill — plus connected-component sizes. | `voids.py <gcode> [layer=60]` |
| `widthdist.py` | **Length-weighted** extrusion-width histogram per role (marker-count stats over-weight short shed corners). | `widthdist.py <gcode> [wall\|gap\|all]` |
| `render.py` | Two generators side-by-side, red = wall-zone gap. Best for locating gaps. | `render.py <gcodeA> [layer=60] [gcodeB] [out.png]` |
| `zoom.py` | Zoomed region drawing every bead as a filled capsule at its **actual `;WIDTH:`**, so you can see whether gap-fill beads truly span their gap. | `zoom.py <gcode> [layer] [cx] [cy] [half] [out.png]` |
| `overlap.py` | **Cross-role double-extrusion**: pairwise footprint intersection between every role pair, with a ¼-nozzle-eroded **BODY** column that strips the expected boundary seam and leaves genuine bead-on-bead overlap (e.g. sparse infill re-extruding over a gap-fill bead). | `overlap.py <gcode> [layer\|all]` |
| `beaddiff.py` | **Before/after visual diff** of one layer from two gcode files, every bead a capsule at its true `;WIDTH:`, role-coloured on a shared scale, with isolated short paths highlighted and counted. The image to attach to a PR. | `beaddiff.py <before> <after> [layer] [out.png] [cx cy half] [--short=0.8]` |
| `layerplot.py` | **The model's cross-section against the beads laid into it** — the one question G-code cannot answer on its own. Needs the JSON `dump_layer` writes (below). | `layerplot.py <layer.json> <out.png> [cx cy half]` |
| `wallbands.py` | **Wall-band anatomy of one island**: labels every island on a layer, then zooms one and draws its wall loops in print order (outer, inner-1, inner-2, …) as separate colours, so "between the two inner walls" is unambiguous. | `wallbands.py <gcode> <layer> [island] [out.png]` |

### Examples

```bash
# Is any wall overlapping on layer 60?
python3 tools/gcode-analysis/coincident.py /tmp/arachne.gcode 60

# How much wall-zone void remains, arachne vs classic?
python3 tools/gcode-analysis/voids.py /tmp/arachne.gcode 60
python3 tools/gcode-analysis/voids.py /tmp/classic.gcode 60

# Are walls printed at full width or shed thin?
python3 tools/gcode-analysis/widthdist.py /tmp/arachne.gcode wall

# Locate the gaps (red) side-by-side, then zoom into one at (x,y) with a ±3mm window
python3 tools/gcode-analysis/render.py /tmp/arachne.gcode 60 /tmp/classic.gcode /tmp/layer60.png
python3 tools/gcode-analysis/zoom.py  /tmp/arachne.gcode 60 -11.5 -1 3.5 /tmp/hull.png

# Which roles double-extrude over each other (e.g. infill over gap fill)? Compare to classic.
python3 tools/gcode-analysis/overlap.py /tmp/arachne.gcode all
python3 tools/gcode-analysis/overlap.py /tmp/classic.gcode all

# Did my change actually fix the layer? Before/after, beads at true width —
# this is the image to attach to the PR.
python3 tools/gcode-analysis/beaddiff.py /tmp/before.gcode /tmp/after.gcode 41 /tmp/diff.png
python3 tools/gcode-analysis/beaddiff.py /tmp/before.gcode /tmp/after.gcode 201 /tmp/rail.png 0.9 -12 2.2 --short=1.5
```

### Seeing the model, not just the toolpath

Every script above reads G-code, so none of them can show what the *model* had
there — and a bead that stops short of a rib's tip, or a thin feature that got no
bead at all, looks perfectly healthy in a plot of the toolpath alone.
[`tests/dump_layer.rs`](../../tests/dump_layer.rs) writes one layer's island
contours and beads (with per-vertex widths) as JSON straight out of the
generator, and `layerplot.py` draws the two together:

```bash
DUMP_MODEL=Filament_Card_Caddy_25.stl DUMP_LAYER=21 DUMP_ROT=45 DUMP_NOZZLE=0.4 \
  DUMP_OUT=/tmp/layer.json cargo test --test dump_layer -- --ignored
python3 tools/gcode-analysis/layerplot.py /tmp/layer.json /tmp/layer.png 81 95 5
```

`DUMP_ROT` turns the plate, which is the cheapest way to tell a property of the
model from a property of its angle to the coordinate grid.

> **Attach the picture to the PR.** Slicing changes are geometry; a diff and a
> table do not let a reviewer see whether the toolpaths are right. See
> [`.github/instructions/slicing-visual-verification.instructions.md`](../../.github/instructions/slicing-visual-verification.instructions.md)
> for the full contract, including why a **centerline** plot must never be used
> to verify one (two beads 0.3 mm apart look separate as lines and overlap as
> material).

> **Reading `overlap.py`.** Wall×surface and wall×infill body-overlap is largely
> the *designed* `infill_overlap_percent` bond and shows up in **both**
> generators — the signal is the **arachne − classic delta** and any
> **gap-fill** pair (`Gap infill × …`), which is pure Arachne and should be near
> zero. A large `Gap infill × Sparse infill` means the infill region was not
> clipped against the gap-fill footprint.

## Example output

`render.py` — Arachne vs. Classic on one layer, red = leftover wall-zone gap:

![Arachne vs Classic wall-zone gaps](examples/gaps-vs-classic.png)

`zoom.py` — every bead drawn as a filled capsule at its true `;WIDTH:`; the green
gap fill tapers to fill the space between the grey walls:

![Beads at real width](examples/bead-widths.png)

A tight zoom on a hull wall (`zoom.py … 60 -11.5 -1 3.5`):

![Gap fill zoom](examples/gap-fill-zoom.png)

## Assumptions & caveats

- **G-code markers:** paths need `;TYPE:` role comments and, for width-aware
  scripts, `;WIDTH:<n>mm` comments (the generator's default annotations).
- **Layers are Z-bucketed** (`round(z, 2)`) so Z-lift travel moves don't shatter
  the model into pseudo-layers — layer indices are dense print layers, not raw
  Z changes.
- **Nozzle/resolution are hardcoded** in `voids.py` (`NOZ = 0.40`, `RES = 0.08`
  mm/cell) and `gap_max = 2.5×nozzle` throughout. Edit the constants for other
  nozzles.
- `voids.py` implements fill-holes / dilate / erode by hand (numpy only, no
  scipy) — fine for a Benchy-sized layer, not tuned for huge plates.
- E-per-mm measured from raw G-code is unreliable (retraction / absolute-E
  bookkeeping); trust the width markers and the capsule render instead.

## See also

- [src/walls/README.md](../../src/walls/README.md) — wall generators (Classic / Arachne).
- [src/gcode/README.md](../../src/gcode/README.md) — G-code emission and the volumetric flow balance.
- [`src/core/README.md`](../../src/core/README.md) — the pipeline order and the
  infill/surface boundary these measurements are about.
