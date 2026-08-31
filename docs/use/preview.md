# Reading the preview

The G-code preview shows exactly what the nozzle will do — not a render of your
model, but the actual toolpaths. Two minutes here saves a failed print.

Press `P` to switch between the model and the preview.

## Colour by

The **Colour by** dropdown repaints the whole preview. Each mode answers a
different question.

| Mode | Answers |
| --- | --- |
| **Role** (default) | What is each line _for_ — wall, infill, support, bridge? |
| **Speed** | Where does it slow down? Sudden slow patches often mean cooling limits. |
| **Flow** | How much plastic per second — the real load on your hotend. |
| **Line Width** | Where beads get thin or fat. Useful for checking thin walls. |
| **Layer Height** | Varies only if you're doing something clever with layers. |
| **Acceleration** | Where the machine is being pushed. Ringing usually starts here. |
| **Fan Speed** | Cooling, per layer. |
| **Temperature** | Per-layer nozzle temperature. |
| **Layer Time** | Which layers are so quick they won't have cooled. |

In **Role** mode the legend doubles as a filter. Click a role to hide it —
hiding infill to inspect the walls underneath is the most common move. Travel
moves and seams are hidden by default; turn them on to hunt down stringing or a
seam landing somewhere visible.

Roles are grouped so you can toggle a whole family at once: **Shell**,
**Fill & Surfaces**, **Support & Adhesion**, **Movement & Markers**.

## Moving through the print

- **Layer slider** — or `↑` / `↓` for one layer at a time.
- **Show all layers** vs **current layer only** — the stack, or one slice in
  isolation. Isolation is better for looking at a specific problem.
- **Progress slider** — scrub within a layer. `→` / `←` step move by move, so
  you can follow the nozzle through a tricky bit.
- **Hover** any line for its role, layer, Z height, width, height and speed.

## What to look for

**Before your first print of a model:**

- The **first layer** covers what it should and has your skirt or brim.
- **Bridges** actually span a gap rather than hanging in air — switch to Role
  and look for bridge-coloured lines.
- **Supports** touch what needs supporting and nothing that doesn't.
- The **top surface** is continuous, without stray dashes or gaps.

**When something printed badly:**

- Blobs or zits → look at **seam** placement.
- Stringing → turn on **travel** and look for long moves across open space.
- Poor overhangs → check **fan speed** and **layer time** on those layers.
- Ringing → check **acceleration** and **speed** around corners.

## The stale-preview rule

The **Re-Slice** button turns amber the moment you change a setting or move a
model. The preview you're looking at is from before that change. Re-slice before
you trust it.
