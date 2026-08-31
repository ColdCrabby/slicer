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

That's the layout on a computer or a tablet. A phone gets the same tools in a
one-handed arrangement — see [On a phone](#on-a-phone) at the end.

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

On a touchscreen, two fingers pinch to zoom and slide to pan — both at once, as
you'd expect. Twisting rotates the view, but only if you clearly mean it: once
you've started pinching, the view won't tip just because your wrist turned. Rest
your hand on the glass while drawing with a pencil and it's ignored.

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

Messages appear bottom-left — top of the screen on a phone, where the sheet
isn't. Info and success fade after a few seconds; errors stay until you dismiss
them. Longer jobs get a progress strip at the top that turns into a notification
when it's done.

## Moving between Home, Slice and Settings

When you first open the app it shows the Cold Crabby logo with a progress bar
beneath it. The logo appears immediately as a soft, low-detail version and
sharpens a moment later once the full artwork has downloaded. The bar follows
the real download, so it tells you how much is genuinely left.

The rail on the left switches between the three areas. The app loads each one
the first time you open it, so on a slow connection the first visit can take a
moment.

When it does, you'll see it: a thin amber line runs along the top of the work
area, and the rail item you're heading to shows a spinner. Nothing appears for a
switch that's already instant — which is most of them, because the app quietly
fetches the other areas in the background once it's finished starting up.

If the app was updated while your tab stayed open, a part it hasn't loaded yet
may have already been replaced on the server. Rather than failing silently, it
offers you a **Reload** banner. Take it — a reloaded tab is a consistent one.

## Undo

`Ctrl`/`⌘ + Z` undoes, `Ctrl`/`⌘ + Y` (or `⌘ + Shift + Z`) redoes. This covers
what you do to the plate — moving, rotating, scaling, adding, deleting,
arranging.

On a touch device without a keyboard, undo and redo buttons appear in the 3D
view toolbar so you can step through history without a shortcut. They show up
automatically on touch tablets and phones; force them on or off any device in
**Settings → General → Controls**.

Settings and profile edits are **not** undoable. They're saved deliberately, and
changing a value back is the undo.

## Making it yours

**Settings → Appearance** has light / dark / system, and an accent colour: the
default molten amber, five other presets, a custom colour, or — on macOS and
Windows — whatever your system accent is set to.

**Settings → General** has the graphics knobs: field of view, anti-aliasing,
render resolution and preview detail. Turn them down on a weak GPU, up on a good
one.

## On a phone

The same app, rearranged for one hand. There is no separate mobile version and
nothing to install — open the slicer's address in your phone's browser.

```
┌─────────────────────────────┐
│ 🦀  My plate              ● │  ← title bar
├─────────────────────────────┤
│        [ tools ]            │
│                             │
│                             │
│         3D plate            │
│                             │
│                  Objects 2  │
│  ┌───────────────────────┐  │
│  │ model.stl     ▸ Slice │  │  ← slice sheet
│  └───────────────────────┘  │
├─────────────────────────────┤
│  Home      Slice    Settings│  ← tab bar
└─────────────────────────────┘
```

- **Navigation moves to the bottom**, where your thumb already is.
- **Slice lives in a sheet across the bottom** of the plate, with the file name
  and the download / send buttons beside it. After slicing, the G-code inspector
  opens inside that sheet and scrolls — Slice stays put.
- **Print settings are a drawer.** Pull the tab on the left edge, or tap it. Tap
  anywhere outside to close.
- **The objects list is folded** to a chip showing how many models are on the
  plate. Tap it to open the list; a warning triangle appears on the chip if
  something can't print where it sits, so a folded list never hides a problem.

Two things are left out rather than squeezed in: the **viewport cube** (drag it
with what?) and the **projection toggle**. Reset-view — the house icon — still
frames the plate in one tap, and both come back the moment you open the slicer
on something wider. Everything that actually gets a model sliced is there.

Turned sideways, the chrome shrinks to give the plate the height back: the tab
bar drops its labels and the panels get shorter.
