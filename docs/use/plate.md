# The build plate

A plate is a **build plate, not a file**. It starts with one model and takes as
many more as fit. Everything on it is sliced into one G-code file.

## Adding models

Four ways, all equivalent:

- Drag files onto the window
- The **add model** button in the tool cluster
- **Open Model to Slice** on the home screen
- Reopening a recent plate, which brings all its models back

Adding never clears what's already there. Only **Empty Workplate** does that.

Each new model is placed clear of the ones already on the bed, using the same
spacing rules as **Place objects**, so a plate never opens with parts sitting
inside each other.

::: details Advanced — multi-part 3MF
A 3MF is a scene, not a model. Cold Crabby expands one into **one object per
build item**, named from the file, all sharing the uploaded bytes. Duplicating
an object shares them too — ten copies of a model cost one upload.
:::

## Moving things around

Pick a tool (or press its key), then drag the handles in the 3D view or type
exact numbers into the card that appears underneath.

| Tool | Key | Card gives you |
| --- | --- | --- |
| Select & move | `M` | X / Y position in mm, **Centre on bed**, **Drop to floor** |
| Rotate | `R` | X / Y / Z in degrees, **Reset rotation** |
| Scale | `S` | Per-axis or linked, as % or as a target size in mm, **Reset scale** |
| Pull to floor | `F` | Click any face to make it the bottom |

**Pull to floor** is the fastest way to orient an awkward part: find the face
that should sit on the bed, click it, done. No arithmetic about which axis to
rotate.

**Gravity** (`G`) keeps objects resting on the bed after every move. Turn it off
if you deliberately want something floating — for example when you're checking
a support-free overhang.

On a touch screen you can skip the handles for a simple reposition: tap a model
to select it, then drag it straight across the bed. Dragging anywhere else
orbits, so nothing moves unless you picked it first.

### Editing several objects at once

Hold `Ctrl`/`⌘` or `Shift` and click to add models to the selection, or use
`Ctrl`/`⌘ + A` for all of them. Without a keyboard, turn on **Multi-select** in
the tool cluster and every tap adds or removes one.

Select more than one and the card keeps working:

- **Position** applies as an offset from the selection's shared centre, so the
  arrangement holds its shape instead of collapsing onto one coordinate.
- **Rotation** and **scale** apply to each object about its own centre.
- **Size in mm** measures each object separately, so a mixed batch all reaches
  the size you asked for.

The header reads `3 objects` for a batch, and shows nothing for a single one —
every duplicate shares a filename, so naming it wouldn't tell you which.

## Placing everything automatically

**Place objects** (`A`) lays the whole plate out in one go. Its card has:

- **Auto-orient** — turn each part onto a sensible face first.
- **Gap** — how much room to leave between parts, in mm.
- **Preferred angle** — shown read-only, because it belongs to the printer, not
  the plate. Many CoreXY machines print best at 45°. Change it in
  **Settings → Printers**. It's only applied when auto-orient runs.

This is one command, not two. There's no separate "orient everything" button
that would fight with the arrangement.

## Duplicating and deleting

Right-click a model — or press and hold it on a touch screen — for **Duplicate**,
**Drop to floor**, **Centre on bed** and **Remove**, right where the model is. If
you have several selected, the menu acts on all of them.

The objects panel has the same **Duplicate** and **Remove** on each row. There,
Remove asks once before it takes effect.

Duplicates are cheap — they share the original's geometry.

## Warnings

Two badges can appear on an object:

- **Outside the build area** — part of it is beyond the print volume. Checked
  against your printer's actual bed shape, not just a box.
- **Overlaps another object** — its footprint intersects another part's. Parts
  touching edge-to-edge don't count, so a tight auto-arrangement stays clean.

Both are warnings. Slicing still runs — the check is an estimate and refusing
would be more annoying than useful — but you should look before you print.

## Naming and reopening plates

The plate's name is taken from the first model and can be edited in the title
bar. The home screen lists recent plates with thumbnails; clicking one restores
its models and layout.

::: details Advanced — where a plate lives
In the browser, plate history is stored locally. On a self-hosted server it's
kept server-side, so any machine on your network can reopen it. Uploaded model
bytes are held per plate, which is how a plate with five files reopens with all
five.
:::

## Printing parts one at a time

**Process → Objects → Print order** offers two modes:

- **By layer** (default) — every part grows together, layer by layer.
- **By object** — each part is finished completely before the next starts,
  front to back.

By-object printing needs headroom: the gantry must clear everything already
printed. Cold Crabby checks your printer's clearance height and radius and warns
about parts that look too tall or too close together. It warns rather than
refuses, because those clearances are estimates.

Whether the machine can **cancel an individual object** mid-print is a printer
setting, not a process one — two printers can share a process but differ in
firmware support. With it on, the G-code declares every part up front, so
Mainsail, Fluidd and OctoPrint list them and let you cancel one that has failed
while the rest of the plate carries on.
