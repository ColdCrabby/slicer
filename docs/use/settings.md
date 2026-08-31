# Print settings

Cold Crabby splits settings three ways, the same way established slicers do.
Knowing which tab something is in is most of the battle.

| Tab | Describes | Changes when |
| --- | --- | --- |
| **Printer** | The machine | You buy a printer or change the nozzle |
| **Filament** | The spool | You swap material |
| **Process** | How to print this thing | Every print, potentially |

Each tab has a profile dropdown at the top. Pick a saved profile and every
option below it fills in. Managing those profiles is covered in
[Printers, filaments and profiles](/use/profiles).

::: tip Can't find something?
Press `Ctrl`/`⌘ + F` and type. The search spans all three tabs, so you don't
have to guess which one owns it.
:::

## The five settings that matter most

If you change nothing else, understand these.

**Layer height** (Process → Layer) — how thick each slice is. Default `0.2 mm`.
Smaller means smoother and slower; larger means faster and more visible layer
lines. Stay at or below about 75 % of your nozzle diameter.

**Walls** (Process → Walls) — how many perimeter loops around the outside.
Default `3`. This, more than infill, is what makes a part feel solid. Going from
2 to 4 walls does more for strength than doubling infill.

**Infill density** (Process → Infill) — how much material inside. Default
`20 %`. Decorative parts are fine at 10 %; functional parts want 30–50 %. Above
about 50 % you're usually better off adding walls.

**Temperatures** (Filament → Temperature) — nozzle `210 °C` and bed `60 °C` by
default, which suits typical PLA. Your filament's label wins over any default.

**Supports** (Process → Support) — off by default. Turn them on when your model
has overhangs steeper than about 45°. Check the preview afterwards: supports
that touch nothing are wasted plastic and a worse surface.

## Everything else, by group

### Printer

| Group | What lives there |
| --- | --- |
| **Hardware** | Nozzle diameter, bed size and shape, kinematics, gantry clearances, whether the firmware can cancel a single object |
| **Retraction** | How far and how fast filament is pulled back on travel; Z-hop |
| **Output** | G-code flavour (Marlin or Klipper), start and end scripts, lifecycle markers |

### Filament

| Group | What lives there |
| --- | --- |
| **Temperature** | Nozzle and bed, with separate first-layer values |
| **Cooling** | Fan speeds, minimum layer time |
| **Filament G-code** | Custom G-code for this material |

### Process

| Group | What lives there |
| --- | --- |
| **Layer** | Layer height, first-layer height |
| **Walls** | Wall count, wall generator, thin walls, extra perimeters, ordering, seam behaviour |
| **Extrusion** | Line widths and flow |
| **Infill** | Density, pattern, angle |
| **Support** | On/off, type, density, overhang threshold |
| **Speed** | Per-role print speeds and travel speed |
| **Quality** | Bridging, dimensional compensation, other accuracy options |
| **Surfaces** | Top and bottom solid layer counts, surface fill, ironing |
| **Adhesion** | Skirt, brim, raft |
| **Objects** | Print order, G-code run between objects |
| **Thumbnail** | The preview image embedded in the G-code file |
| **Mesh** | How the incoming model is interpreted |

Options that only apply in certain configurations hide themselves. Choosing the
classic wall generator, for example, reveals options the Arachne generator
doesn't use — so the panel never offers you a control that would do nothing.

## Infill patterns

| Pattern | Character |
| --- | --- |
| **Rectilinear** (default) | Parallel lines, alternating direction each layer. Fastest. |
| **Grid** | Lines crossing at right angles. Stronger, slower. |
| **Honeycomb** | Hexagons. Good strength for the material spent. |
| **Gyroid** | A 3D curve. Equal strength in every direction; nice for flexibles. |
| **TPMS-D** | Diamond minimal surface. Organic and isotropic. |

## Two special modes

**Spiral (vase) mode** (Process → Walls) prints a single continuous wall that
climbs as it goes — no seam, no layer changes. For open, single-walled models
only. Turning it on forces the settings it's incompatible with (extra walls,
infill, top layers, retraction) off for you, and keeps your bottom layers as the
base.

**Ironing** (Process → Surfaces) makes a second, hot, barely-extruding pass over
top surfaces to smooth them. Slow, and only worth it on visible flat tops.

::: details Advanced — tuning the ironing pass
**Type** chooses what gets swept: every top surface, only the single highest one
(much faster on a tall model, and usually the only face anyone sees), or all
solid surfaces.

**Flow** is how much material the pass adds, as a percentage of a normal bead —
around 10 % is enough to re-melt the surface without raising it. **Spacing** is
how far apart the passes run; well under a bead width is what flattens the
ridges between them. **Speed** should stay low, because the nozzle needs dwell
time to melt what it crosses. **Angle** defaults to following the layer's own
fill direction; set an explicit angle to cross the fill instead, which flattens
it more effectively.
:::

## Getting parts to the right size

A printer lays a bead slightly wider than asked, so parts come out a little
large and holes a little tight. Both are consistent for a given machine, so both
can be measured once and corrected (Process → Quality).

**XY size compensation** grows or shrinks every contour by a fixed amount. Print
a test cube, measure it, and set the difference as a negative value if the cube
came out oversized. Because the material spreads inward as well as outward, this
also tightens holes.

**Hole compensation** adjusts holes on their own, so a peg that will not fit can
be freed without changing the outside of the part.

Both default to off. Start from a measurement, not a guess — and keep the values
small; a shrink larger than a thin feature will erase it, which the slice log
warns you about.

**First layer height** (Process → Extrusion) prints the bottom layer thicker
than the rest. The extra material absorbs what mesh bed levelling only
approximates, which is why almost every profile sets it. It has no effect when
you print on a raft, since the raft takes over contact with the bed.

## Where your settings are saved

Changes in this panel apply to the current plate. To make them permanent,
save them into a profile — see
[Printers, filaments and profiles](/use/profiles).

::: details Advanced — configuring outside the UI
The CLI and self-hosted server read a layered `slicer.toml`: built-in defaults,
then your user config, then a project `slicer.toml` in the working directory,
then command-line flags. Each layer deep-merges over the last, so a project file
only needs the values it changes. See
[Configuration](/teams/configuration).
:::
