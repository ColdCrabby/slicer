# Changelog

All notable changes to Slicer Engine are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

This file is the **single source of truth** for release notes. It is embedded into
every build (CLI, WS server, WASM/UI, desktop) at compile time and republished
verbatim as the body of each GitHub Release. See [RELEASING.md](RELEASING.md) for
the workflow that keeps those in sync.

<!--
Maintainers: keep an `## [Unreleased]` section at the top. When cutting a release,
rename it to `## [x.y.z] - YYYY-MM-DD` and add a fresh empty `## [Unreleased]`
above it. The `release` skill (say "cut a release") automates this — it curates
these notes and acknowledges contributors. `scripts/gen-changelog-draft.sh` and
`scripts/release-contributors.sh` provide the raw material if you do it by hand.

Style: keep each entry to one to three tight lines (what it does, its default,
the one number worth quoting) — deep rationale lives in AGENTS.md and the module
READMEs, not here. Break a long category into `#### Theme` groups so it stays
scannable, keep the bold **Feature name** lead on every bullet, and never put
issue/PR numbers or repo links in the notes. See the tone rules in
.claude/skills/release/SKILL.md for the full voice.
-->

## [Unreleased]

### Added

- **Library.** Every model that reaches a plate is kept once, with a picture,
  so the next plate is built from what you have. Duplicates — renamed copies,
  re-exports — are recognised; the selected model turns in a live preview.
  With a plate open, the library opens beside it, and picking a model puts it
  straight on the plate.
- **Copy, link or both.** On the desktop, choose whether models are copied into
  the library or linked where they are, and point it at folders to watch. On
  iPad the library is a folder in Files, so a model saved there just appears.

### Fixed

- **The same model now slices to the same G-code.** Simplifying a region before
  Arachne's medial fill could leave two of its edges crossing, and the Voronoi
  diagram of crossing edges is undefined — so two runs of one file could print
  different walls there. Regions are now made clean again after simplifying, and
  the surface trim no longer depends on hash order. Classic was never affected.
- **Removing a model can be undone.** Undo re-adds it from its file, in place;
  `Delete` / `Backspace` now remove the selection too.
- **Upload & print asks first, and never sends a stale file.** Starting a print
  always confirms, and after any change the result button waits for a re-slice.
- **The layer counter matches the slice.** The preview no longer counted a
  layer twice when moves sat between its two layer-change markers.
- **Arrow keys stay in the field you're typing in.** The G-code preview's arrow
  shortcuts no longer steal them, and single-key plate shortcuts only work while
  the plate is on screen.
- **Dragging the empty plate no longer opens the file picker.**
- **Selecting behaves the way the platform does.** `⌃`-click on a Mac opens the
  menu instead of adding to the selection, `⌘`-click adds there and `Ctrl`-click
  elsewhere, and a modifier-click that misses no longer throws the selection
  away. Clicking a selected part keeps it selected.
- **Duplicates and new models arrive selected**, and a duplicated group lands
  beside itself instead of on top of its neighbours. **Centre on bed** moves a
  selection as one piece instead of stacking it.
- **The objects list's menu matches the model's** and acts on the whole
  selection; `Shift`-click selects a range.
- **`Esc` and a tool's own key put the tool down**, back to Select & move.
- **A printer whose connection type isn't supported says "Not supported"** rather
  than "Offline", and a switched-off printer is grey rather than red.

### Added

- **Export a plate as 3MF.** Right-click the plate's tab, or an empty spot on
  the bed, and choose **Export as 3MF…** — every model is saved where it sits,
  ready to reopen here or in another slicer. The CLI gains `--export-3mf`.
- **Cancel a running slice** — the Slice button reads Cancel while it works.
- **Filament weight and cost after a slice**, next to the print time.
- **A Model | G-code switch** at the top right, with names instead of an icon
  that only appeared after the first slice.
- **More in the macOS menu bar** — File › Add Model, Slice and Export G-code,
  Settings, and a Help menu. The desktop window also reopens at the size and
  place you left it.
- **Keyboard: `⌘/Ctrl + D` duplicates, `⌘/Ctrl + Enter` slices.**
- **Drag a model to move it.** With a mouse, Select & move now picks a part up
  and moves it in one gesture; empty bed still orbits. Touch keeps tap-then-drag.
- **Box selection.** `Shift`-drag adds everything the box touches, `⌥/Alt`-drag
  takes it away; with Multi-select on, the pencil draws the box while fingers
  keep orbiting.
- **Nudge with the arrow keys** — 1 mm, `Shift` for 10, `⌥/Alt` for 0.1, in the
  direction you're looking.
- **Zoom to a part** with `Z`, a double-click, or **Zoom to** in the context
  menu — without swinging the camera round.
- **A 90° quarter turn per axis** on the Rotate card.
- **A pen's eraser end erases support paint**, whatever the brush is set to.
- **A quiet note while slicing for a generic printer**, with a link to add yours.

- **Inner walls have their own speed** — the hidden walls behind the surface no
  longer inherit the visible one's pace. `Inner Wall Speed` defaults to 125% of
  the outer wall, stated as a percentage so slowing the outer wall for a better
  finish keeps the buried ones fast.
- **Internal solid infill has its own speed** — the solid layers sealed between
  the skins are no longer priced like the top surface they share a name with.
  `Internal Solid Speed` defaults to 150% of the top surface, which lands on the
  same speed as sparse infill.
- **Travel brakes gently onto the outer wall** — a hop that lands where a
  visible wall starts now slows at the outer-wall acceleration instead of the
  travel one, so the toolhead is not still ringing when the wall begins. On by
  default; every other hop keeps full travel acceleration.

### Changed

- **The app starts faster.** The first screen downloads about a fifth less code
  (943 kB → 726 kB, 242 kB → 192 kB compressed). The settings schema, unused
  form controls and a second popover engine no longer load before the home
  screen.
- **Settings, regrouped for a glance.** The sidebar splits into **App** and
  **Library**; each library page shows how many you have and which one is the
  default. General's twenty rows move to **3D View** and **Controls** (keyboard
  shortcuts included), each preference is one line with the rest behind its ⓘ,
  and on/off choices are switches.
- **Search all of Settings** from the sidebar — pages, preferences, your
  profiles by name, and every printer, filament and process setting.
- **Built-in profiles read as editable.** Rename one where its name is, see
  *Saved* after each change, and put the shipped values back with **Restore
  defaults**; no more "Duplicate to customise".
- **The profile outline fits on an iPad.** Tighter columns, and the section list
  folds itself to icons when that is what makes the room. Its left edge is now
  one continuous line that steps in under open sections, with the part of the
  editor on screen drawn on it exactly as you scroll.
- **Place objects nests by shape and by height, not by bounding box.** A plate
  prints a layer at a time, so one part may take the space above or below
  another wherever the two never want the same height in the same place — a part
  leaning at 45° hangs over its neighbour, a small part tucks in under a flared
  rim. Parts with flat overhangs still keep the column beneath them, because
  support material would be there, and printing one part at a time gives every
  part its own space from the plate up. The gap you set is measured between the
  parts themselves, overhead as well as sideways, and parts may take a quarter
  turn to fit; `Turn to fit` on the placement card turns that off. A plate with
  room to spare comes out as one compact group in the middle instead of a long
  row, so the nozzle travels less between parts.
- **After a slice, the result action leads.** Download, Upload or Print becomes
  the main button; Re-Slice steps back until something changes.
- **One Help menu in the title bar** replaces five link icons, which on a phone
  left the plate's name two letters wide.
- **Recent plates come first on Home**, and each printer card opens that printer.
- **Debug overlays and the thumbnail animation are off by default** — turn them
  on in Settings → 3D View.
- **The browser build's performance note is a one-time notice** with a link to
  the desktop app, not a dialog in front of the model you just opened.
- **"Save to profiles"** replaces "Sync", and the app says **plate** throughout.
- **The print-settings drawer has a Done button** on phones and tablets, and the
  object list no longer covers the Slice card beside a docked panel.

- **The stock profiles are quicker across the board.** Outer walls run at
  120 mm/s (was 80), top surfaces at 100 (was 60), bridges and the steep overhang
  bands that inherit from them at 25 (was 10), and travel at 300 mm/s (was 150).
  A stock 3DBenchy drops from 46m39s to 36m51s — a fifth of the print — at the
  same layer height, wall count and infill.
- **The fast presets moved up with them.** High Speed walls go to 180 mm/s and
  Maximum to 250, both still held below their own infill speed so there is
  something left to spend on the surface. Past roughly 20 mm³/s the limit is the
  hotend rather than the profile — set `Max Volumetric Speed` on the filament.
- **Touch targets are sized for a fingertip on touch devices** — one size for
  finger and Apple Pencil alike, so switching between them never shifts the
  layout.
- **Tablet chrome is calmer** — the blanket 44 pt floor is now two numbers: 40 px
  for an isolated control and 36 px for a settings row, which takes roughly a
  screen and a half of scrolling out of the settings panel on an iPad.
- **The sidebar's resize edge behaves like it does on a desktop** — its enlarged
  touch strip overhung the plate and turned taps beside the panel into accidental
  resizes.
- **G-code Layer and Progress read as one pair** — Progress gains its own header
  and a `137 / 138` readout of where you are inside the layer, so both sliders
  line up instead of one sitting indented behind a label.
- **The preview's step arrows are 32 px and no longer stacked** — forward-a-layer
  and forward-an-extrusion sat a few pixels apart at the same spot, and they now
  carry a resting surface so a touchscreen can see they are buttons.
- **Holding a preview arrow runs through the layers**, accelerating as it goes,
  the same way holding a number field's `+` already did.
- **One place for the app to talk to you** — anything about the plate now
  appears as a pill at the top of the scene, next to the work it describes. A
  running job fills its own pill and finishes in it, instead of handing the
  result to a message in the opposite corner.
- **The bottom-left corner is yours again** — the floating message stack no
  longer covers the object list or the "outside the build area" warning.
- **Errors say their piece where they happened** — a preset that will not
  import says so in the picker, a reset that fails says so on its own card, and
  a slice reports the reason it failed on the Slice button's status line
  instead of repeating the whole event somewhere else.
- **The celebration is for a print actually starting**, not for every upload it
  used to play alongside an identically worded message.

### Fixed

- **Curves no longer stutter or print as facets.** Spiral (vase) loops used to
  reach the printer unsimplified, as thousands of 0.01 mm zig-zag moves that
  made Klipper slow down at nearly every vertex. Every path now merges those
  micro-segments first, and the default path tolerance drops from 0.05 mm to
  0.0125 mm so large arcs stay round rather than faceted.

- **Held steppers no longer die under a fingertip.** A touchscreen reads a long
  press as a request for a context menu about half a second in — right after the
  repeat started — which stopped `+` / `−` from running at all on a phone or
  tablet, and popped the system callout on top of the button being held.
- **Auto-oriented parts sit on the bed.** A part turned onto an angled face
  could float tens of millimetres above the plate, land off-centre, or be flagged
  out of bounds when it fit. Placement now measures the part itself, not a box
  around it.
- **Rotate, scale and Pull to floor turn a part where it stands.** Models
  exported from CAD often carry an origin far from the geometry, and turning
  about it swung the part across the plate. Pull to floor also no longer sinks
  the rest of the part into the bed when the picked face isn't the lowest.
- **Pull to floor highlights the face you'll get.** On finely tessellated
  curves the highlight could spread round half the model; it now stops where
  the surface stops being flat.
- **A model opened with auto-orient off lands on the bed**, centred, rather
  than wherever its file's origin put it.

## [0.5.0] - 2026-09-16

Support structures grow up, thin features finally print, and a plate stops being
a file. This release makes overhangs printable without hand-holding, fixes the
walls that quietly vanished on anything rotated or narrow, and turns workplates
into real tabs that survive closing the app.

### Highlights

- **Supports that hold, not hint** — sloped overhangs get real columns, the gap
  closes to 0.35 mm, islands print as continuous loops, and a brush lets you
  enforce or block support anywhere the automatic rule guesses wrong.
- **Thin features print, at any angle** — ribs, fins and dividers survive a wide
  nozzle and slice identically turned 45° as flat. A card caddy that lost 42 % of
  its dividers when rotated now lays the same length at every angle.
- **Workplates are tabs** — open several plates at once, switch between them in
  any runtime, and find them arranged exactly as you left them after a restart.

### Added

#### Supports

- **Paint support where you want it** — `B` picks up a brush to **enforce**
  support regardless of angle or **block** it entirely. **Support Auto** turns
  the angle rule off, for plates supported only where painted.
- **Support line width** joins the other per-role widths, without dragging the
  raft's deliberately coarser bead along with it.

#### Printing and output

- **Fuzzy skin** — an outer-wall texture that roughs the surface with a small
  random jitter. Tunable thickness and spacing, off by default, purely cosmetic.
- **Pause and colour change at a layer** — mark any layer in the preview to stop
  for an insert or a filament swap. Emits Marlin `M0`/`M600`, Klipper `PAUSE` or
  RepRap `M226`.
- **Bed mesh levelling** — `bed_mesh_mode` emits `BED_MESH_CALIBRATE` or
  `G29`/`M420 S1` at print start, off by default. `bed_mesh_adaptive` bounds
  recalibration to the print's own footprint.
- **Minimum layer time** — `min_layer_time_s` slows a short layer so it has time
  to cool, clamped at `min_print_speed` (10 mm/s). First layer exempt, off by
  default.
- **Acceleration for every role** — inner wall, sparse and solid infill, gap
  fill, support and travel each carry their own limit, all shipping with
  non-zero defaults (10 000 mm/s² baseline, 15 000 for travel, 1 000 for first
  layer and bridges). Set any to `0` for the old defer-to-firmware behaviour.

#### Printers and plates

- **Your Klipper printer sets itself up** — detection reads the machine's own
  configuration: build volume, kinematics, nozzle, velocity and acceleration
  limits, pressure advance, firmware retraction, object cancellation and which
  start macros it uses. **What we read from your printer** shows every value and
  the `printer.cfg` section it came from.
- **The wizard only asks what your printer can't answer** — a short question or
  two after detection, each with an answer already picked and a line saying why.
- **Two faster presets, and a CoreXY to run them on** — **High Speed** (200 mm/s)
  and **Maximum** (300 mm/s, 30 000 mm/s²), both holding the outer wall and top
  surface back. A **Generic CoreXY 350 mm** preset carries the machine side.
- **Sync to profile** — review every setting a plate has changed against its
  profiles, side by side like a diff, and write the ones you keep straight back.
- **See when a colleague changes the plate you are on** — on a shared server,
  saving tells everyone else looking at it. Nothing reloads by itself.
- **Open a model straight from another app** — Cold Crabby registers for `.stl`,
  `.obj` and `.3mf` on Windows, macOS, Linux, iPhone and iPad. The model joins
  the plate you have open rather than replacing it.

#### The app

- **An outline for the settings panel** — press the list button or
  `Ctrl`/`⌘ + Shift + O` for a table of contents: every section and every
  setting by name, including what the panel folds away. The profile editors get
  the same rail with a filter box.
- **Settings start calm and open all the way** — each section shows what a print
  depends on, then an `Advanced` row that expands in place; a second press
  reveals Expert. Process drops from eleven sections to seven. Search still
  reaches everything.
- **Every setting says what it measures** — units, sensible steps and fractions
  read as percentages, so a nozzle steps by 5 °C rather than to 210.01.
- **The preview re-slices itself, when that's worth doing** — Automatic
  re-slices a second after you stop, but only while slices stay under 5 s, timed
  per plate.
- **The slice dock says how long the print will take**, read from the G-code's
  own commanded speeds.
- **Hold `+` or `−` to run a number up or down**, accelerating the longer you hold.
- **Thumbnails can match the 3D view's look** — Settings → General → Thumbnail
  look renders with the viewport's shading and shadow. Plain by default.

### Changed

- **Better infill and surface defaults** — sparse infill is now **TPMS-D** at
  20 %, which carries load in every direction, and top and bottom surfaces are
  **rectilinear** for an even, evenly-pressed face. First layer slows to 25 mm/s.
- **Smooth-bridge defaults** — bridges print at **10 mm/s** with **1.5×** flow so
  their strands fuse into a continuous floor before they sag.
- **Dynamic overhang speed actually slows the transition** — the 25–50 % band a
  curved hull spends many layers inside is now stated as a percentage of the
  preset's own wall speed, and any band can be expressed that way. **Slow down
  curled perimeters** no longer needs a hand-tuned override to engage.
- **Auto-orient goes for the biggest face that actually touches the bed** — real
  bed contact instead of every downward-facing surface, so flat-bottomed parts
  stop being tipped onto a corner. A pose too tall for the machine ranks last.
- **A thin rib prints as the wall it is** — wall speed, wall acceleration, wall
  colour, instead of the gentle gap-fill treatment.
- **Short hops inside a part no longer retract** — under 5 mm and crossing no
  wall, the retract, Z-hop and prime are skipped; where there is a way round, the
  hop travels back over its own beads so what it drools lands on the part. One
  rib-heavy model loses 85 % of its retractions.
- **Material settings live with the filament** — flow ratio, maximum volumetric
  speed and pressure advance move from Process, where a profile switch used to
  overwrite your per-spool calibration. A new **Material** group collects what
  the spool is.
- **Every speed reads in mm/s**, travel and retraction included; press the unit
  to switch the whole panel to mm/min. The stored value never changes.
- **Gravity is on by default, and remembered.**
- **Filament type is a list**, offering the materials firmware and other slicers
  read back — while keeping whatever a vendor profile already carries.
- **Deleting a profile confirms inline** instead of asking you to type its name.
- **Touch targets meet the 44 pt floor** across tabs, steppers, menus and the
  viewport cube, and the page no longer zooms out from under a pinch.
- **The G-code editor follows the app theme.**

### Fixed

#### Walls and travel

- **Turning a model no longer changes how it slices** — thin features were
  rebuilt from geometry the 0.01 mm coordinate grid had turned into a staircase.
  A caddy now lays the same divider length at 0° and 45°, to within 1 %.
- **A thin feature survives a wide nozzle** — anything under the minimum bead
  width was dropped outright, so a 0.6 mm nozzle printed a caddy as a solid
  block. It now prints at the minimum width, down to half of it.
- **Thin features print end to end**, and a bead no longer breaks into a file of
  millimetre dabs where the material it follows briefly narrows.
- **A tapering bead keeps its taper over a bridge or overhang** — splitting walls
  there used to discard per-vertex widths for the whole layer.
- **No more strands between thin ribs** — a cavity now counts as air, and every
  hop between ribs dives back through the body rather than reaching for the free
  tip. On a 25-slot caddy, unretracted travel across open air falls from 5.5 m to
  0.8 m.

#### Overhangs and layers

- **Overhang detection measures material, not centrelines** — near-vertical
  funnels, chamfered lips and flaring hulls were getting bridge speed and bridge
  cooling for surfaces landing on solid plastic. Steep-but-touching walls are
  still slowed and cooled, but keep wall flow and stay one continuous loop.
- **Overhangs are classified by geometry, not by rounding** — a uniform 0.34 mm
  ledge used to come back with four alternating verdicts around one circle.
- **Small layers no longer crawl to a blob** — the minimum-layer-time slowdown
  scaled against the general print speed, so bridges, overhang bands and ironing
  fell straight through the `min_print_speed` floor. The floor now applies to the
  speed actually emitted, and nothing dwells to make up a shortfall.
- **Supports work on sloped overhangs** — the threshold angle was inert because
  overhang classification had already retagged the walls the support stage
  measures. A 60° cone goes from nothing to full support; a 30° one is still left
  alone.
- **Support islands print whole** — the segment closing each perimeter loop was
  never extruded, leaving every island open on one side. The same fix closes a
  wall loop that overhangs along its entire length.
- **Support prints as loops, not dabs, and no longer over-extrudes** — it is
  charged at its flow spacing like every other fill role, about 12 % less than a
  full nozzle-width bead.
- **Rafts and skirts account for supports**, which used to start in mid-air just
  above the plate.
- **Supports are switched off in spiral (vase) mode**, where nothing could reach
  them anyway.

#### Workplates and the app

- **Switching tabs actually switches plates** in every runtime, and a plate comes
  back with each model's position, rotation, scale and support paint intact.
- **Your plates survive closing the app** — models are kept on the device beside
  the plate. Room for them is bounded, oldest-unopened first.
- **`+` starts a plate instead of throwing one away**, and a renamed tab keeps
  its name everywhere. Renaming is a double-click; tabs work from the keyboard.
- **Switching back to a plate no longer re-downloads its models**, and reloading
  always gets the current build.
- **Plates holding several models slice correctly everywhere** — the desktop app
  sliced every object out of the first model, and the browser slicer refused
  outright.
- **The app notices when the engine goes away** — a heartbeat spots a dead
  connection within seconds and reconnects; edits made while it is down report
  the problem instead of vanishing.
- **Tapping a model on a tablet selects it** — a parked invisible gizmo hit area
  swallowed taps, and a mouse-sized 4 px tolerance discarded a fingertip's wander
  as a drag. Tolerance is now judged per pointer.
- **The slice progress bar tracks real work** — four phases carried no weight at
  all and froze the bar while they ran.
- **"Use filament colour" honours the colour you set**, in the viewport and in
  the embedded thumbnail.
- **Undo while typing undoes your typing**, not the last thing you did to the plate.
- **Notifications no longer pile up**, dialogs fit on iPhone and iPad, and a
  finished slice reports its own layer count.
- **Closing a workplate tab works.**

### Contributors

Thanks to @max-scopp, who shipped this release end to end — supports, walls,
workplate tabs and the printer wizard all landed in this cycle.

## [0.4.0] - 2026-08-31

The biggest release so far — 181 files and about 15,000 new lines — and it pulls
in two directions at once. Print quality gains the three settings that were
missing or only pretended to work: ironing, dimensional compensation and a real
first layer height. And the app itself stops being desktop-only: an iPad or a
phone now gets a layout, gestures and controls built for it, on a home screen
that draws in a fraction of the time.

### Highlights

- **Ironing, dimensional compensation and a real first layer height** — the
  finishing controls a print actually needs. Ironing re-melts top surfaces flat,
  XY/hole compensation corrects a machine that prints over- or under-sized, and
  the bottom layer is finally sliced *and* extruded at the thickness the profile
  asks for.
- **Elephant-foot compensation that keeps thin detail** — the flare at the base
  of a print is removed without erasing the first layer's fine geometry. Unlike a
  plain shrink, the correction is limited by how thin the model is at each point,
  so embossed text, logo strokes and thin ribs survive where a uniform 0.3 mm
  offset would delete every feature under 0.6 mm.
- **A slicer that fits a tablet and a phone** — an iPad keeps the desktop layout
  but folds panels away from the plate, tap, long-press and drag work with a
  finger or a Pencil, and below 640px the whole interface rearranges into a
  bottom tab bar, a settings drawer and a slice sheet. Previously a 390px screen
  gave the 3D view about 50px of width.
- **A home screen that appears immediately** — 691 kB to first draw, down from
  1.58 MB, with the slicing engine, the 3D viewer and the code editor loaded only
  when you reach for them. Blocked main-thread time on arrival falls from about
  2 s to under 30 ms.

### Added

- **Support structures** — overhangs steeper than the threshold angle (45° by
  default) now get real support geometry instead of a "not implemented" warning.
  Two styles: `normal` grid columns and `tree` branches that converge as they
  descend. Dense interface layers, an XY clearance and a Z air gap keep the
  contact clean enough to snap off.
- **Only from build plate** — restricts support to columns that can reach the
  bed through open space, so nothing lands on the print. Overhangs with no route
  down are left unsupported on purpose. Off by default; on a shelf overhanging a
  wider base it removed every millimetre resting on the model while keeping the
  support beyond it.

#### Print quality

- **Ironing** — the top-surface smoothing toggle now does something. Previously
  it could be switched on in the profile wizard, was documented as working, and
  logged "not yet implemented" at slice time. A near-dry pass re-melts finished
  top surfaces flat, with its own type, flow (10 %), spacing (0.1 mm), speed and
  angle, and its own colour in the preview.
- **Dimensional compensation** — correct a machine that prints consistently
  over- or under-sized. **XY size compensation** offsets every contour;
  **hole compensation** adjusts holes on their own, so a tight press-fit can be
  freed without moving the outside of the part. Both default to off.
- **Elephant-foot compensation** — corrects the flare the bed's squish leaves at
  the base of a print. Unlike a plain shrink it is limited by how thin the
  geometry is at each point, so embossed text, logo strokes and thin ribs on the
  first layer keep their width instead of being erased. It also holds back where
  the model itself flares steeply outward, and switches itself off on a raft. Off
  by default; 0.1–0.2 mm suits most machines.
- **First layer height** — now affects the print instead of only the file
  header. The bottom layer is sliced and extruded at its own thickness, so a
  profile asking for 0.24 mm no longer silently prints 0.2 mm. Skirt and brim
  follow it; a raft suppresses it.

#### Tablets, phones and touch

- **Tablet layout** — an iPad keeps the full desktop arrangement but stops
  leaving panels open over the plate: the G-code inspector and the object list
  both start folded, and the print-settings tab moved out of the middle of the
  screen, where the model is. Opening the settings drawer no longer dims the
  plate behind it — you are peeking at the settings, not leaving the model.
  Applies to any window under 1024px, so Split View and a docked desktop window
  get it too.
- **Phone layout** — the whole UI rearranges itself below 640px instead of
  overflowing. Navigation moves to a bottom tab bar, print settings become a
  drawer with a pull tab on the left edge, and Slice sits in a full-width sheet
  along the bottom with the G-code inspector scrolling inside it. Settings
  sections become a scrollable chip strip and the printer/filament/profile
  editors stack instead of splitting into two columns.
- **Finger-sized controls on touch screens** — buttons, dropdowns, legend chips
  and the layer slider are now sized for a fingertip on every touch device rather
  than only on phones. The layer slider's grip went from 18px to 26px; scrubbing
  through layers on a tablet was the worst of it.
- **A context menu on the plate** — right-click a model, or press and hold it on
  a touch screen, for Duplicate, Drop to floor, Centre on bed and Remove, acting
  on the whole selection when you have one. Holding empty bed offers select all,
  clear, place objects and reset view.
- **Multi-select for touch and pen** — a tool-cluster toggle that makes each tap
  add or remove an object, standing in for the ⌘/Ctrl a tablet has no key for.
  It appears once there are two objects to choose between, and turns itself off
  when it can no longer be reached.
- **Drag a selected model across the bed** — with a finger or a pen, select a
  model and then drag it. Dragging anywhere else still orbits, so nothing moves
  unless you picked it first. The move handles are also drawn larger on touch,
  so a fingertip can pick one axis instead of all three.
- **Fold the G-code legend out of the way** — the inspector now has a header you
  can tap to collapse it to a single row, keeping the layer counter and the Slice
  button in place. Unfolded it sizes itself to the room actually available, so on
  an iPad the whole legend and both sliders are visible without scrolling. It
  starts folded on tablets, phones and narrow windows, open on a large screen,
  and remembers whichever you choose.

#### Loading and startup

- **A loading screen on first open** — the logo now appears immediately with a
  progress bar beneath it, instead of a blank page while the app downloads. A
  tiny embedded copy shows straight away and sharpens once the full artwork
  arrives, and the bar tracks the real download rather than guessing.
- **Visible page loading** — switching between Home, Slice and Settings shows a
  thin progress line and marks the destination in the navigation rail whenever
  the wait is long enough to notice. Anything under 120 ms stays silent, and the
  app quietly pre-fetches the other areas once it has finished starting up.
- **Reload prompt for stale tabs** — if a page can no longer be loaded because
  the app was redeployed under an open tab, the existing update banner now
  offers a reload instead of the click doing nothing.

#### Platform

- **Keep the app on an iPhone or iPad without a Mac attached** —
  `pnpm run ios:install` builds the release app, signs it with a free Apple ID
  and installs it on a connected device, so it keeps working after the dev
  server stops. No paid Apple Developer Program: the trade is that a free
  signature lasts seven days, and re-running with `--renew` resets the clock
  without touching your models, profiles or settings.
- **Preset catalog picking got faster and more honest** — the picker loads a page
  at a time instead of walking the whole category, fetches the complete preset
  when you import it (so its slicing parameters really arrive), and shows a busy
  state on the entry you picked while it does.
- **GitHub links in the title bar and catalog picker** — a direct route to the
  project, and to the source of any catalog preset.

### Changed

#### Speed and size

- **Faster startup** — the app now downloads 691 kB before it can draw, down
  from 1.58 MB. The 3D viewer, the settings workspace and the code editor are
  fetched when you actually open them, not before the home screen appears.
- **The home screen appears without waiting for the slicing engine** — the
  ~750 kB WebAssembly engine and its worker used to load before the first screen
  could draw, even though nothing there needs them. They now load once the
  browser is idle, or the moment you open a model, whichever comes first. Blocked
  main-thread time on arrival falls from about 2 s to under 30 ms, and the page
  weighs 536 kB less.
- **Smaller install and deploy** — the bundled code editor no longer ships ~90
  programming-language grammars and four language servers it never used. Total
  deployed JavaScript falls from 17.7 MB to 8.4 MB.
- **Settings pages open without loading the code editor** — the G-code editors
  on the printer and filament pages sit well below the fold, but still cost
  ~4 MB the moment the page opened. They now load when you scroll to them.
- **The in-app logo is 74% smaller** — 52 kB of PNG became 14 kB of WebP.

#### Interface

- **Touch targets and form rows on phones** — controls grow from 34px to 40px,
  labelled rows stack so a dropdown gets the full width, toasts move to the top
  of the screen (the bottom now belongs to the slice sheet), and the object list
  folds to a chip that still flags a part that cannot print.
- **The object list remembers whether you folded it** — it starts folded on a
  tablet or a narrow window, and unfolding it now sticks instead of resetting on
  the next visit.
- **Two controls are hidden on phones** — the viewport cube, which needs a drag
  it cannot receive, and the projection toggle and operation-pipeline inspector,
  which do not fit alongside the tools that get a model sliced. All three return
  on any wider screen.
- **You can pinch to zoom the interface again** — zoom was disabled across the
  whole page to keep gestures away from the 3D view. Only the 3D view holds onto
  them now, so settings, prose and small print can be magnified everywhere else.
- **Muted text is legible** — the quietest text tone sat below the contrast
  minimum in both themes, and well below it in light mode. It has been darkened
  (light) and lightened (dark) enough to pass on every surface it appears on.
- **"Running in your browser" appears when you open a model**, not the instant
  the page loads. Same message, but it now arrives when you have something to
  slice — and it is no longer the first thing a new visitor meets.

#### Development

- **The dev stack starts on a seeded pair of random ports**, so a second
  worktree, a colleague on the same machine or a parallel agent session never
  collides with yours.
- **The documentation site now wears the app's own design language**, built from
  the same theme tokens the product uses, so the two cannot drift apart.

### Fixed

#### Touch and gestures

- **Tapping a model on a tablet now selects it** — two separate faults made touch
  and Pencil selection fail. An invisible transform-gizmo hit area sat parked at
  the centre of the bed whenever nothing was selected, and swallowed any tap that
  landed on it; that check runs only for touch and pen, so a mouse never saw it.
  Selection was also judged by a mouse-sized 4px tolerance, and a fingertip is a
  ~10mm disc whose reported centre wanders as the skin flattens, so most real
  taps were discarded as drags. Taps are now judged per pointer — 4px for a
  mouse, 9 for a pen, 16 for a finger — and a hidden gizmo no longer intercepts
  anything.
- **Pinching to zoom no longer spins the model on a touchscreen** — rotation is
  measured as an angle, so its noise floor grows as your fingers close; a
  measured pinch-in sprayed ~16° of unwanted roll. Twist now has to be a
  sustained, deliberate turn, and once you have clearly started pinching the
  view is locked against rolling for the rest of the gesture.
- **Slow pinches now zoom** — a gentle pinch moves well under a pixel per event
  on a 120 Hz iPad, and every one of those was discarded rather than banked, so
  the camera simply refused to move. Small movements now accumulate.
- **A palm can no longer hijack a two-finger gesture** — a hand settling while
  two fingers were already pinching was let through, and took over the gesture
  the moment a real finger lifted.
- **Losing a finger no longer freezes the camera** — a touch released over the
  toolbar, or the app being backgrounded mid-pinch, could strand the viewport
  with navigation disabled until the page was reloaded.

#### Plates and slicing

- **Plates holding several models now slice correctly everywhere** — a workplate
  is a build plate, not a file, but only the hosted slicer treated it that way.
  The desktop app sliced every object out of the *first* model, so a second one
  came out as a copy of the first; the in-browser slicer refused outright with
  "Missing mesh bytes". Each object now resolves to the file it was actually
  loaded from, in every runtime.
- **The desktop app now prints the plate you arranged, not the file you opened**
  — models were sliced exactly as their source file defined them, ignoring every
  move, rotation and drop-to-bed. A multi-part 3MF came out with its parts still
  stacked as the authoring tool assembled them, and duplicates or extra parts
  were dropped entirely.

#### Interface

- **The "Print settings" tab no longer flickers when you hover it** — pointing at
  the tab used to open the drawer, which hid the tab, which closed the drawer, in
  a loop. Resting at the very edge of the screen still peeks at the panel; the
  tab itself now only responds to a click.
- **Dialogs no longer collapse in Safari** — a dialog with a body ("Running in
  your browser", "What's New", the operation pipeline) drew as a title with the
  buttons jammed underneath and its content squashed to one clipped line. It now
  opens at its full height and scrolls when taller than the screen. Affected
  iPad, iPhone and the macOS app; the in-browser slicer worst of all, since its
  welcome notice is a dialog.

### Next up

Two big ones are being worked on right now:

- **Support structures** — the last major gap in the pipeline. Overhangs that
  need holding up will finally get held up.
- **Importing your existing profiles** — bring the printer, filament and process
  profiles you have already tuned in another slicer straight into this one,
  instead of rebuilding them by hand.

### Contributors

Thanks to @max-scopp for shipping this release.

## [0.3.0]

The preset catalog goes live. "Pick it from the catalog" in the printer and
filament wizards now browses the real Cold Crabby Preset Cloud instead of an
empty placeholder, so a new profile can start from a genuine vendor preset. Add
touch-friendly undo/redo in the 3D view and a round of Windows desktop polish.

### Highlights

- **Live preset catalog** — the printer and filament wizards now search the
  Cold Crabby Preset Cloud for vendor presets and pre-fill a new profile from
  your pick. Search is ranked server-side across the whole catalog, and if the
  cloud is unreachable the app falls back to its built-in defaults and keeps
  working offline.
- **Undo / redo in the 3D view** — history buttons on the plate toolbar for
  touch devices where the ⌘/Ctrl+Z shortcut can't be reached.

### Added

- **Preset catalog picker** — browse and search real vendor presets when
  creating a printer or filament profile; a pick pre-fills the profile's
  identity fields, tagged with a link back to its catalog source.
- **Undo / redo buttons in the 3D view** — forward and backward history buttons
  on the plate toolbar. Shown automatically on keyboard-less tablets and phones;
  force them on or off in Settings → General → Controls. Auto by default.

### Changed

- **Live accent tracking is event-driven** — the desktop app now updates its
  accent tint the moment you change it in the OS (Windows registry change
  notifications; macOS distributed notifications) rather than catching up on a
  2-second poll.

### Fixed

- **G-code editors render properly in the browser build** — the printer and
  filament G-code boxes could show up as a plain, unstyled text field instead of
  the syntax-highlighting editor, most often on the in-browser (WASM) version.
  The editor now waits for its own styles to load before it appears, so it comes
  up fully formed every time.
- **Undo no longer wipes the plate after navigating** — dip into Settings and
  back, or use the browser's back button, and your objects keep their positions
  and undo history instead of being deleted by the next undo. History now resets
  only when the workplate is genuinely replaced.
- **The "re-slice" hint no longer clears itself** — moving an object or changing
  a setting *while a slice is running* used to be silently absorbed into the
  preview once it finished, so the "Scene changed — re-slice" hint disappeared
  even though the on-screen G-code predated your edit. The comparison baseline is
  now captured the moment you press Slice, so a mid-slice change correctly keeps
  the hint lit until you re-slice.
- **No more flashing console on Windows** — the desktop app used to re-read the
  OS accent colour every couple of seconds by shelling out, which popped a brief
  `cmd` window on Windows on every check. It now reads the accent directly and
  waits for the OS to signal a change instead of polling, so the flickering after
  login is gone and macOS stops re-checking on a timer too.
- **No more launch hang on Windows** — the desktop app used to open a blank,
  unresponsive window for a moment on Windows before it became usable. Its window
  now stays hidden until the interface has actually drawn, so the app appears
  fully rendered and ready the instant you see it.

### Contributors

Thanks to @max-scopp for shipping this release.

## [0.2.0] - 2026-08-30

The build plate grows up. What used to be one model on a plate is now a real
build plate — add as many objects as you like, cancel a failed one mid-print, or
print them one at a time — and every infill pattern finally deposits the exact
density you ask for at any line width.

### Highlights

- **Multi-object build plates** — place several models on one plate, manage them
  in a new Objects panel, cancel a single failed part from Mainsail/Fluidd, or
  print parts sequentially front-to-back. Multi-part 3MFs land as separate, named
  objects instead of one fused blob.
- **Infill that hits its density** — line spacing and per-line flow now come from
  the real extrusion width, not a hardcoded 0.4 mm. Grid stopped printing double,
  honeycomb tiles and stacks like it should, and a 0.6 mm nozzle asked for 20 %
  no longer prints ~13 %.

### Added

#### Infill & surfaces

- **Advanced infill options** — infill anchors (weld sparse lines to the wall and
  merge broken dashes into one continuous move — the biggest quality win here),
  sparse-infill layer combining (print shared infill once at a stacked height),
  internal solid layers, separate top/bottom/internal surface patterns (including
  monotonic for a cleaner top finish), and a bridging-angle override. All off, or
  matching the previous behaviour, by default.
- **Five more infill patterns** — `aligned-rectilinear`, `triangles`,
  `tri-hexagon`, `cubic` and `concentric`. OrcaSlicer's pattern names are accepted
  as-is, so imported profiles map without a translation table.
- **Spiral (vase) mode** — `spiral_vase` (CLI `--spiral-vase`) prints a single
  continuous outer wall whose Z ramps smoothly over each layer, for a seamless
  single-wall vase with no Z-seam. Off by default.

#### Multi-object build plates

- **Cancel one object mid-print, or print objects one at a time** — the plate now
  tracks which part every extrusion belongs to. *Exclude object* wraps each part
  in firmware markers (Klipper `EXCLUDE_OBJECT_*`, Marlin / RepRapFirmware `M486`)
  so a failed part can be cancelled from Mainsail, Fluidd or OctoPrint while the
  rest of the plate carries on. *Sequential printing* (Print order → by object)
  finishes each part front-to-back, lifting clear of everything already on the
  bed, with clearance checks reported as warnings and optional between-object
  G-code. Both off by default; with both off the plate slices exactly as before.
- **Multiple objects per workplate** — a plate is now a build plate, not a single
  file. An **Add model** button and a multi-select picker place more models
  without replacing what's there, a new **Objects panel** lists and manages them
  (flagging anything out of bounds or overlapping), and reopening a saved plate
  restores every object. Multi-part 3MF files now land as separate, named objects
  instead of one fused blob.
- **One placement command** — a single **Place objects** tool replaces the rival
  "auto-orient" and "arrange all" buttons that undid each other's work. It sits
  with move / rotate / scale and opens a card for the two things worth varying
  (auto-orient and gap). The same settings apply when you drop a model in.
- **Preferred print orientation, per printer** — set a diagonal angle (e.g. `45°`
  for CoreXY) applied to every auto-oriented part after it's laid on its best
  face. Defaults to `0°` (untouched).
- **Multi-object CLI slicing** — `slice -i part_a.stl -i part_b.stl` builds one
  plate. A new `--arrange` flag (with `--arrange-spacing` and
  `--arrange-auto-orient`) packs it without overlap, and the transform flags apply
  to every loaded model.

#### Printer & firmware output

- **Chamber temperature management** — the filament asks for a chamber temperature
  (with a hotter first-layer soak) and the printer says whether it can deliver it;
  only when both agree are directives emitted, with a safe soak sequence and
  Klipper's native commands. A start G-code that already heats the chamber keeps
  ownership.
- **Z offset** — a per-printer `z_offset_mm` (Settings → Printers → Hardware)
  added to every Z coordinate written to the G-code, so it works on any firmware
  with no macro to maintain. Same meaning as in PrusaSlicer and OrcaSlicer.
- **Dynamic overhang speed & cooling** — perimeter segments are graded by how much
  hangs over unsupported air, and each degree prints at its own speed with extra
  part-cooling. On by default.
- **Advanced retraction modes** — firmware retraction (`G10`/`G11`), relative
  extruder distances, minimum-travel-before-retract, restart-extra prime,
  retract-on-layer-change, and wipe-while-retracting. All default to the previous
  behaviour.
- **Perimeter routing & ordering options** — `external_perimeters_first` (outer
  wall now printed last by default, matching PrusaSlicer/Orca/Cura),
  `extra_perimeters`, `thin_walls` (classic generator), and
  `ensure_vertical_shell_thickness` and `avoid_crossing_perimeters`. Defaults
  preserve existing behaviour except the inner-first ordering.
- **Volumetric-flow limiter** — `max_volumetric_speed` (mm³/s) caps the feedrate
  so the hotend is never asked to melt faster than it can, per-segment for
  variable-width beads. Defaults to `0` (unlimited).
- **Geometry-aware acceleration** — `outer_wall_acceleration` and
  `bridge_acceleration` add role-specific limits on top of layer-type
  acceleration (lower outer-wall for less ringing, low bridge for steady flow).
  Default `0`.
- **Layer-type acceleration control** — `acceleration`, `first_layer_acceleration`
  and `top_surface_acceleration` emit a firmware acceleration command when the
  target changes. Default `0`.
- **Pressure / linear advance output** — a non-zero pressure-advance value is
  emitted once after the start script in the correct firmware form (Klipper
  `SET_PRESSURE_ADVANCE`, Marlin `M900 K`). `0` disables it.
- **G-code metadata header** — every program opens with a flavor-specific metadata
  block: slicer version and timestamp, model name, layer count, height, filament
  usage, time estimate and bounding box. A new `filament_density_g_cm3` drives the
  weight.

#### Models & profiles

- **Automatic mesh repair on import** — holes are capped, cracked vertices welded,
  inside-out triangles turned right-side-out, and zero-area or duplicate triangles
  dropped. The UI raises a toast, the CLI logs a warning, and a new
  `mesh-check` command prints a full report without slicing. Clean models are
  never touched; pass `--no-mesh-repair` to slice the raw geometry.
- **Export your profile library** — Settings → General downloads every printer,
  filament, print profile and label as TOML (a ZIP bundle, or a single
  `profiles.toml` the engine reads directly). Printer API keys are stripped, so
  the export is safe to share.
- **Settings tell you when they depend on something else** — a chamber temperature
  set for a printer with no chamber heater now says plainly it won't take effect
  and links to the fix. The CLI prints these warnings too.

#### App, platform & tooling

- **Release notes inside the app** — Settings → What's New lists every release,
  newest first, with the version you're running highlighted. The post-upgrade
  dialog shows that same list.
- **iPadOS / iOS target** — the Tauri shell now builds and runs on iPad with the
  full slicing engine on-device. The `pnpm run ios:*` scripts drive the toolchain,
  Xcode project generation, and a live-reload simulator build.
- **Slice diagnostics & bed-type tracking** — a `bed_type` setting is recorded in
  the header, and the `slice` CLI reports model height, filament usage and the
  estimated print time.
- **Live versioning** — every build reports its true version, derived from git
  tags. Development builds report `development`.
- **Embedded changelog** — bundled into every target; the UI shows a "What's New"
  dialog the first time it runs after an upgrade.
- **Releases pipeline** — tagging `vX.Y.Z` builds all targets and publishes a
  release whose notes are taken from this file.
- **Commit revision in build info** — every build records the exact short commit
  hash, shown in Settings → General and reported by the CLI `info` command.

### Changed

#### Infill accuracy & patterns

- **Infill density is now accurate at every line width** — line spacing and the
  flow charged for each line both come from the real extrusion width, not a
  hardcoded 0.4 mm reference. A 0.6 mm nozzle asked for 20 % used to print ~13 %.
- **Grid infill no longer prints double** — it laid two full-density passes
  instead of two half-density ones. Honeycomb cells and the gyroid period are on
  the libslic3r relations now too, and all scale with the line width.
- **Honeycomb is a real hexagonal tiling** — continuous zig-zag walls drawn once,
  instead of stamped hexagons with every shared wall drawn twice.
- **Honeycomb cells stack again** — it (and triangles, tri-hexagon, cubic) was
  rotated 90° every other layer and keyed to the region's bounding box, so walls
  landed on the layer below's voids. Consecutive Voron-cube layers now share 79 %
  of their infill geometry, up from 2 %.
- **TPMS-D actually prints now** — its segments are chained into continuous curves
  and the period recalibrated, so it deposits the full requested density instead
  of about a seventh.
- **Top surfaces default to monotonic line, bottoms to monotonic** — a cleaner,
  direction-consistent finish that also stopped 106 mm² of top-surface material
  printing over the inner wall on a Voron cube.

#### Docs & dependencies

- **The documentation now leads with the product, not the architecture** — a
  proper guide to *using* Cold Crabby plus a teams track for self-hosting and
  configuration, with the engineering docs slimmed to a map. A banner notes the
  docs are early and their structure may still change.
- **Dependency maintenance** — cleared the Dependabot backlog: the Angular
  front-end moves to the 22.x line on TypeScript 6.0, the Rust engine to
  `sea-orm` 2.0 and `reqwest` 0.13, plus the grouped npm bumps. No behavioural
  change to sliced output.

### Fixed

#### Print quality

- **Your filament's cooling settings are now actually used** — Fan Speed, Bridge
  Fan Speed, First Layer Fan Speed and Fan Off For First Layers were shown and
  saved but ignored by the generator, so the part-cooling fan ran during the first
  layer for every material, quietly costing bed adhesion. Fan Speed is now the
  cooling ceiling, and the bridge and overhang boosts are held back on the layers
  where cooling is off.
- **Isolated infill specks in narrow wedges** — a connected infill region too
  small to hold more than one dash (2 mm² at a 0.4 mm nozzle) is now skipped. It's
  an **area** rule, so a genuinely thin cavity that deserves a lattice keeps every
  line.
- **Generator-specific wall options are now hidden for the generator that ignores
  them** — `thin_walls` / `wall_distribution_count` (classic) and
  `gap_fill_min_length_mm` (Arachne). This also stopped `thin_walls` from silently
  deleting Arachne's thin features — turning it off had wiped ~50 slot fins on a
  filament card caddy. Arachne now always prints thin features.
- **Top-surface "squiggles" where solid fill grazes a wall** — a surface meeting
  the wall band at a shallow angle was filled with a dense micro-serpentine of
  sub-millimetre stubs. Solid surface regions narrower than one extrusion width
  are now dropped before filling, while thicker geometry keeps its exact shape and
  sharp corners.
- **Arachne "splat" gap-fill and gap fill under top surfaces** — isolated
  sub-millimetre gap-fill beads (≈270 on a 3DBenchy) that cost a full
  retract/travel/un-retract each are dropped, and a redundant bead running under a
  thin solid strip is pruned with the surface filling the strip in its place.
- **Tiny sparse-infill "splat" dashes** — `solid_regions` is now grown by one bead
  before being subtracted from the infill area (killing the crescent slivers the
  scanline shattered into dashes), and `min_infill_extrusion_mm` also filters
  sparse infill. Together with the gap-fill fixes this cut isolated sub-0.8 mm
  extrusions on a 3DBenchy by ~76 %.

#### Interface & import

- **The documentation site failed to build** — a line of prose wrapped an inline
  code span across a line break, leaking its placeholders out as raw, unclosed
  HTML.
- **The transform panel was blank whenever more than one object was selected** —
  it now edits the whole selection: **Position** shifts every part by the same
  amount (keeping your layout), while **Rotation**, **Scale** and **Size** apply
  per part about each one's own centre.
- **The arrange gap defaulted to 0 mm on a fresh install** — an unset preference
  read back as `0` rather than "unset". It now starts at 4 mm.
- **iPad Apple Pencil + two-finger navigation "spazzing"** — the viewport's
  pointer arbiter now classifies a whole gesture group at once, so a two-finger
  pan/pinch is never split into a stray single-finger camera rotate.
- **3MF models loaded at the wrong scale** — the importer ignored the
  `<model unit="…">` declaration and read every coordinate as millimeters. All six
  spec units are now normalized on import.
- **Viewport-cube ortho snap popped back to perspective on pan/zoom** — only a
  genuine rotate now breaks the flattened, dimension-true view; panning and
  zooming keep it, so you can inspect a snapped view up close.

### Contributors

Thanks to @max-scopp, who built everything in this release. Onward to 0.2.0.

## [0.1.0] - 2026-08-23

### Added

- Initial slicer engine: STL/OBJ/3MF loading, mesh slicing, Arachne
  variable-width wall generation, top/bottom surface detection, and infill.
- Unified scene engine (single source of truth for object placement) shared by
  the CLI, WebSocket server, and WASM UI.
- Angular UI with a Three.js viewport and G-code preview, plus a Tauri desktop
  shell.
- Command-line interface with `slice`, `info`, and schema-generation commands.
