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

## Changes belong to the plate

Everything you see in the panel comes from the three profiles you picked. When
you change one of those values, the plate remembers **that one change** — not a
copy of every setting.

You can see which ones they are: a changed setting's name is *italic*, and its
section gets a dot, so a collapsed **Walls** still tells you something inside it
was touched. A strip pinned to the bottom of the panel counts
them and offers **Reset all**, which hands every changed setting back to its
profile.
Setting a single value back to what the profile says does the same for that one
— it stops being a change and starts following the profile again.

This matters for a reason that only shows up later. Edit a print profile — say
you decide 4 walls, not 3 — and every plate using it prints with 4 walls,
including the ones you already tuned. The only settings that stay put are the
ones you deliberately changed on that plate.

Your changes are saved to the plate as you make them; the panel says so briefly
underneath. Come back to a plate a week later, from the tab bar or your history,
and it opens with the printer, filament and profile it was set up with, where
you put each model, and the changes you made on top — even if you have been
printing something else in PETG since.

Where that is saved depends on how you run Cold Crabby, and it is the same rule
as your profiles: on the server if you self-host, on the machine if you use the
desktop app, and in the browser only if you use the web version. Clearing your
browser does not cost you your plates unless the browser is all you have.

::: details Advanced — what actually gets sliced
Cold Crabby tells the slicer *which* printer, filament and print profile you
picked — by name, not by sending copies of them — plus your list of changes. The
slicer already has your profiles, and combines them itself: engine defaults,
then printer, then filament, then process, then your changes, each winning over
the one before. The combining happens in one place, so the command line, the
desktop app and the browser cannot disagree about what a profile means.

Naming them rather than sending them is also what makes an edit stick. If every
slice shipped a copy of your profiles, the last browser to slice would quietly
overwrite a change you made in another tab, or that a colleague made on a shared
slicer.

The one large thing still sent every time is the preview picture embedded in
your G-code, because it is a picture of *your* 3D view — your camera angle, your
theme, your filament colour — and only your browser can draw it.
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

Supports normally stand wherever they are needed, including on the model
itself. **Only from build plate** restricts them to columns that can reach the
bed through open space. Anything that would have to rest on the print is
dropped, so those overhangs print unsupported — a deliberate trade you make
when a support landing on the model would scar a surface you care about, or sit
somewhere you could never get a tool into. Expect to lose coverage: on a shelf
overhanging a wider base, the part beyond the base is still supported and the
part above it is not.

## Advanced and Expert

Each section shows the settings a print actually depends on, then a quiet
**Advanced** row at the bottom with a count — `Advanced 10`. Pressing it expands
that section in place, so you keep your scroll position and your context; a
second press reveals **Expert**. Nothing moves, and there is no "expert mode" to
switch the app into.

The split is about whether you can form an intention, not about how experienced
you are. Seam position is Advanced because you can want the seam at the back.
A bead-transition threshold is Expert because almost nobody can predict what
changing it does — including people who have been printing for years.

Two things worth knowing:

- **Search ignores all of this.** Type into the settings search and you reach
  every setting the slicer has, whatever tier it sits in and whichever tab owns
  it. If you know the name — including the name another slicer uses for it —
  that is the fastest way there.
- **Anything you have changed stays visible**, even if it lives in Expert. A
  setting you can't find is a setting you can't put back.

Whole sections work the same way. A few hold nothing but advanced settings —
Quality, Thumbnail, Time estimate — so they are not listed until you ask for
them; a section header that opens onto nothing is worse than no header. The
**Advanced sections** control at the bottom of the list brings them in, which is
what keeps Process at seven sections rather than eleven.

Each section, and the list itself, remembers how far you opened it — so if you
work in Advanced you only say so once.

If you always want everything in view, **Settings → General → Settings detail**
sets where the panels open: *Standard*, *Advanced*, or *Everything*. It moves
the starting point only — the per-section controls still open further, search
still reaches everything, and nothing is hidden from you at any level.

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
| **Material** | What the spool *is* — type, name, colour, diameter, density, cost |
| **Temperature** | Nozzle and bed, with separate first-layer values |
| **Cooling** | Fan speeds, minimum layer time |
| **Extrusion** | Flow ratio, maximum volumetric speed, pressure advance — the numbers you calibrate per spool |
| **Filament G-code** | Custom G-code for this material |

### Process

| Group | What lives there |
| --- | --- |
| **Layer** | Layer height, first-layer height |
| **Walls** | Wall count, wall generator, thin walls, extra perimeters, ordering, seam behaviour, fuzzy skin |
| **Infill** | Density, pattern, angle |
| **Support** | On/off, type, density, overhang threshold, interface layers, clearances, whether support may only start from the build plate |
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

### The thumbnail in your G-code

Most printers show a preview of the print on their screen, and that picture is
embedded in the G-code file. **Process → Thumbnail** decides what goes in it:
its size, the camera angle it's shot from, a light, dark or transparent
background, and the model's colour.

How the model is *rendered* is not a print setting — the picture is taken in the
3D view, on your machine — so it sits with the other graphics options under
**Settings → General → Thumbnail look**. Plain, the default, shoots it flatly,
which means the same plate previews identically wherever it's sliced. Match this
view borrows your own scene's shading, gloss and contact shadow instead.

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

**Fuzzy skin** (Process → Walls) roughs up the outer wall with a random,
hand-textured bump instead of a smooth surface — useful for hiding layer lines
or giving a part a deliberately organic look. It's purely cosmetic: inner
walls and infill print exactly as they would otherwise, and turning it off
always reproduces the original smooth output.

::: details Advanced — tuning the texture
**Thickness** is how far each bump can push in or out, in mm — larger values
give a coarser, more pronounced texture. **Point distance** is how closely
spaced the bumps are along the wall — smaller values pack in more, finer
detail.
:::

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

### The flare at the very bottom

**Elephant foot compensation** fixes a different problem from the two above. The
first layer is deliberately squashed into the bed to make it stick, so it
spreads sideways and only the base measures oversize — enough that a part won't
sit flat, or won't drop into the hole it was designed for. XY size compensation
would shrink the whole part to fix the bottom of it.

Measure the bulge with calipers, halve it, and put that in. 0.1–0.2 mm covers
most machines. Only the first layer is corrected, because only the first layer
is squashed.

::: tip It won't eat your first-layer detail
Shrinking the first layer sounds like it should wipe out embossed text and thin
logo strokes — that's what a plain shrink does. This one measures how thin the
geometry is at each point and simply stops there, so a fine feature keeps its
width while the walls around it are corrected in full. It also leaves the base
alone where the model already flares outward above it, and skips itself entirely
when you print on a raft, where nothing touches the bed.
:::

::: details Advanced — tuning the correction
**Elephant foot layers** spreads the correction over more than one layer,
ramping it to zero. Leave it at 1 unless a large correction leaves a visible
step at the second layer.

**Minimum contour width** is the width the correction will never shrink a
feature below. Left at 0 it works this out from your wall width; raise it to
protect chunkier detail, lower it for a more literal correction.
:::

**First layer height** (Process → Layer) prints the bottom layer thicker
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
