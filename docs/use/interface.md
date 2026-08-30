# The interface

One screen does the work: a 3D view of your build plate, with panels around it.
Nothing is hidden behind menus.

```
┌──────────────┬─────────────────────────────────────┬──────────────┐
│              │            viewport cube            │  G-code      │
│   Settings   │                                     │  inspector   │
│    panel     │           3D build plate            │              │
│              │                                     │  Objects     │
│  Printer     │                                     │  on the      │
│  Filament    │          ┌───────────────┐          │  plate       │
│  Process     │          │  tool cluster │          │              │
│              │          └───────────────┘          │  ▸ Slice     │
└──────────────┴─────────────────────────────────────┴──────────────┘
```

## The 3D view

The bed, the print volume, and your models. Navigate it the way you'd expect:

- **Drag** to orbit, **scroll** to zoom, **right-drag** (or two fingers on a
  trackpad) to pan.
- **Click** a model to select it, `Esc` to deselect, `Ctrl`/`⌘ + A` to select all.
- The **viewport cube** top-right snaps the camera to a face, edge or corner.
  Click its centre to return home.
- `Shift + Space` switches between perspective and orthographic. Orthographic is
  the honest one when you're checking whether something is square.

On a trackpad or tablet, a two-finger swipe orbits by default. If you'd rather
it panned, change it in **Settings → General → Controls**. Palm rejection is on
by default for pen input.

## The tool cluster

Floating under the model, this is where you manipulate what's on the plate.

| Tool                | Key | What it does                                        |
| ------------------- | --- | --------------------------------------------------- |
| **Select & move**   | `M` | Drag handles to move; also plain selection           |
| **Rotate**          | `R` | Spin around an axis                                  |
| **Scale**           | `S` | Resize, uniformly or per axis                        |
| **Pull to floor**   | `F` | Click a face; that face becomes the bottom           |
| **Place objects**   | `A` | Auto-arrange everything on the bed                   |
| **Add a model**     |     | Same as dropping a file in                           |
| **Gravity**         | `G` | Objects drop to the floor after every move           |
| **Model / preview** | `P` | Switch between the model and the sliced G-code       |

Picking a tool opens a small card beneath it with numeric fields — exact
position, rotation in degrees, size in millimetres or percent. Type a number if
dragging isn't precise enough.

The plate-editing tools disappear in G-code preview. There's nothing to edit
there, and a change you can't see happen is worse than no change.

## The settings panel (left)

Three tabs — **Printer**, **Filament**, **Process** — each with a profile
dropdown at the top and collapsible groups of options below.

`Ctrl`/`⌘ + F` jumps to the search box. Type "infill", "seam", "brim" and the
groups filter down. This is usually faster than remembering which tab something
lives in.

Full tour: [Print settings](/use/settings).

## The objects panel (right)

Every model on the plate, with its triangle count and size. Select from here
when the 3D view gets crowded.

Watch for the warning badges: *outside the build area*, and *overlaps another
object*. Both are warnings, not blocks — you can still slice, but you probably
shouldn't.

Each row also has **Duplicate** and **Remove**. Remove asks once ("Click again
to remove") before it does anything.

## The slice button (bottom right)

Press **Slice**. Afterwards it becomes **Re-Slice**, and turns amber when you've
changed something since the last slice — so a stale preview always looks stale.

Below it, a status line: `Ready to slice` → `Slicing…` → `Sliced · N layers ·
1h 12m`, or a red failure with the reason.

Once it succeeds, the result button lets you **Download**, **Just upload**, or
**Upload & print**. It remembers which you used last.

## The G-code inspector (right, after slicing)

Appears when you switch to preview. Colour the toolpaths by role, speed,
temperature and more; hide path types you don't care about; scrub through layers
and through individual moves. See [Reading the preview](/use/preview).

## Notifications

Messages appear bottom-left. Info and success fade after a few seconds; errors
stay until you dismiss them. Longer jobs get a progress strip at the top that
turns into a notification when it's done.

## Undo

`Ctrl`/`⌘ + Z` undoes, `Ctrl`/`⌘ + Y` (or `⌘ + Shift + Z`) redoes. This covers
what you do to the plate — moving, rotating, scaling, adding, deleting,
arranging.

Settings and profile edits are **not** undoable. They're saved deliberately, and
changing a value back is the undo.

## Making it yours

**Settings → Appearance** has light / dark / system, and an accent colour: the
default molten amber, five other presets, a custom colour, or — on macOS and
Windows — whatever your system accent is set to.

**Settings → General** has the graphics knobs: field of view, anti-aliasing,
render resolution and preview detail. Turn them down on a weak GPU, up on a good
one.
