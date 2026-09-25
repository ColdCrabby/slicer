# Keyboard and gestures

All of this is also in the app under **Settings → Controls → Keyboard shortcuts**.

`⌘` on macOS, `Ctrl` everywhere else.

## Editing

| Action | Key |
| --- | --- |
| Undo | `⌘/Ctrl + Z` |
| Redo | `⌘/Ctrl + Y` or `⌘/Ctrl + Shift + Z` |
| Place objects on the bed | `A` |
| Select all objects | `⌘/Ctrl + A` |
| Duplicate the selection | `⌘/Ctrl + D` |
| Remove the selection | `Delete` or `Backspace` — undo brings it back |
| Slice the plate | `⌘/Ctrl + Enter` |
| Put away a card, or clear the selection | `Esc` |

`Esc` closes whatever floats over the plate — the placement card, the brush
popout, the print-settings drawer — and clears the selection.

Shortcuts stand down while you are typing in a field, so they can't reach past
your caret — that includes the G-code arrows — and the plate shortcuts only work
while the plate is on screen. Clicking the plate hands the keyboard back.

## The tool panel

| Action | Key |
| --- | --- |
| Jump into the open tool panel | `Tab` |
| Back out to the plate | `Esc` |

`Tab` from the plate lands on the part of the card you actually came for — the
**X** field with Move, Rotate or Scale, the **brush mode** with Paint, the
**Place** button with Place objects, where `Enter` then runs it. From there `Tab`
walks the rest of the card as usual, and `Esc` puts you back on the plate.

## Object tools

| Action | Key |
| --- | --- |
| Move | `M` |
| Rotate | `R` |
| Scale | `S` |
| Pull a face to the floor | `F` |
| Paint supports | `B` |
| Brush size and mode, at the pointer | `Shift + B` |

## View

| Action | Key |
| --- | --- |
| Toggle gravity | `G` |
| Model ↔ G-code preview | `P` |
| Orthographic ↔ perspective | `Shift + Space` |

## G-code preview

| Action | Key |
| --- | --- |
| Next / previous layer | `↑` / `↓` |
| Next / previous extrusion | `→` / `←` |

## Number fields

Every numeric field takes more than typing:

| Action | Input |
| --- | --- |
| Step up / down | `↑` / `↓`, or scroll the wheel while hovering |
| Coarse step (×10) | Hold `Shift` |
| Fine step (×0.1) | Hold `Alt` / `⌥` |
| Run up or down | Hold `+` or `−` — it repeats, and speeds up as you hold |

Hover-scrolling is the fast one — you don't have to click into the field first.
Holding works on a touchscreen too, and the preview's layer and progress arrows
run the same way.

## Search

| Action | Key |
| --- | --- |
| Focus settings search | `⌘/Ctrl + F` |
| Search open workplates | `⌘/Ctrl + Shift + A` |

## Workplates

The same keys as tabs in a browser — in the desktop and iPad apps.

| Action | Desktop and iPad app | Browser |
| --- | --- | --- |
| New workplate | `⌘/Ctrl + T` | `Alt + T` |
| Close workplate | `⌘/Ctrl + W` | `Alt + W` |
| Close all workplates | `⌘/Ctrl + Shift + W` | `Alt + Shift + W` |
| Reopen a closed workplate | `⌘/Ctrl + Shift + T` | `Alt + Shift + T` |
| Next / previous workplate | `Ctrl + Tab` / `Ctrl + Shift + Tab` | `Alt + Shift + →` / `←` |
| Go to workplate 1–8, or the last | `⌘/Ctrl + 1`…`9` | — |

A browser keeps `Ctrl + W`, `Ctrl + T` and the rest for its own tabs, and no
page can take them over — so in the browser the same actions sit on `Alt`
(`⌥` on a Mac). Pressing `Ctrl + W` there anyway asks before the page closes
while you have workplates open.

On a Mac, the desktop app also lists these under **File** in the menu bar.

## Mouse

| Action | Input |
| --- | --- |
| Orbit | Left-drag |
| Pan | Right-drag |
| Zoom | Scroll |
| Select | Click a model |
| Context menu | Right-click a model, a workplate tab, or a list row |

## Touch and pen

| Action | Gesture |
| --- | --- |
| Orbit | One finger |
| Orbit or pan | Two-finger swipe — pick which in **Settings → Controls** |
| Zoom | Pinch |
| Context menu | Long-press |
| Add to or remove from the selection | Long-press a model → **Add to selection** / **Remove from selection** |

**Palm rejection** is on by default. Once you've used a stylus, a resting hand
stops moving the camera. Turn it off in **Settings → Controls** if you don't use
a pen.

Without a keyboard, undo and redo appear as buttons in the 3D view toolbar
instead of shortcuts. They show automatically on touch devices; you can force
them on or off in **Settings → Controls**.

On iPad, long-press opens the system action sheet rather than an in-app menu —
it's the OS's own control, so it behaves the way the rest of iPadOS does.
