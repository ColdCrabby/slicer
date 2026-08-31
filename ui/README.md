# Cold Crabby — Web UI

The Angular front-end of Cold Crabby. It uploads meshes, drives the slice, renders the live G-code preview, and lets you tweak settings — all against a single Rust core that also powers the CLI.

It exists for one reason: **what you preview in the browser must be exactly what slices on the server.** Both run the same Rust code. The UI compiles part of the engine to WebAssembly so scene placement is computed locally, and delegates the heavy slicing to the server over WebSocket.

There is now an alternative `web-slicer` build for fully local slicing in the
browser. It keeps the default WS-backed flow untouched, but swaps `slicer.ts`
to use `SceneHandle.sliceGcode()` when the UI is built with
`pnpm run hydrate:web-slicer` plus `pnpm run ui:build:web-slicer`. That mode
requires a wasm-capable `clang++` toolchain because the slicing pipeline pulls
in `clipper2`.

---

## The contract

```mermaid
flowchart LR
    user[(User)]
    ui["Angular shell<br/>(this app)"]
    wasm["scene-wasm<br/>(Rust → WebAssembly)"]
    ws["WebSocket session<br/>/api/ws"]
    server["Rust server<br/>(serve command)"]

    user -->|drag mesh| ui
    ui -->|SceneOp| wasm
    wasm -->|render buffer| ui
    ui -->|Slice / SceneSnapshot| ws --> server
    server -->|Progress · SliceComplete| ws --> ui
```

- The Angular app **never reimplements** scene math. Translate, rotate, drop-to-floor, align-face — every gesture becomes a `SceneOp` and is applied by the Rust scene engine compiled to WASM. See [src/scene/README.md](../src/scene/README.md) for the SSOT contract.
- **A plate holds objects from several files, so every object carries its own `source_id`** and resolves it through [`ModelSourceRegistry`](src/app/services/model-source/model-source-registry.ts). Never answer "which model does this object slice from?" per *plate* — by list position, by "the first upload", or by one path for everything. Every runtime that did got it wrong: the browser slicer refused to slice a second model, the desktop app sliced it as a copy of the first.
- Schemas and TypeScript types are **generated from the Rust definitions**, not hand-written. See "Generated artifacts" below.
- The G-code preview is decoded from the same `SliceResult` produced by the CLI's `slice` command.

---

## Anatomy

```
ui/src/app/
├── app.config.ts          providers (router, http, markdown, input-modality, keyboard-shortcuts)
├── app-routes.ts          /, /slice/(new|:requestUuid), /settings/* — all lazily loaded
├── pages/
│   ├── home/              landing dashboard
│   ├── slice-new/         upload + initial slice
│   └── slice-viewer/      G-code preview, layer scrubber, history
├── nexus/                 application shell — top bar, sidebar, layout, print-estimates
├── components/            stateless building blocks
│   ├── viewer/            three.js canvas + ViewportCube + 3D-view-toolbar
│   ├── code-editor/       Monaco editor wrapper (lazy-loaded; used by transmit-preview panel)
│   ├── settings-panel/    schema-driven forms
│   ├── file-upload/       drag-and-drop, progress, upload-guard hook
│   ├── history-panel/     past slice runs from the server's SQLite ledger
│   ├── status-panel/ connection-state/ notification-center/ logo/
│   └── …                  card, list-history, viewport-cube
├── services/
│   ├── scene-engine.service.ts       wraps the WASM SceneHandle (single instance)
│   ├── scene-command/scene-command.ts  single dispatch point for SceneOps; gesture-batching + history
│   ├── scene-history/scene-history.ts  linear undo/redo stack (max 50 snapshots)
│   ├── keyboard-shortcuts/             global Ctrl+Z / Ctrl+Y undo/redo hotkeys
│   ├── editor-panel.ts               toggle signal for the transmit-preview panel
│   ├── slicer.ts                     high-level slice orchestration
│   ├── slicer-connection.ts          WebSocket transport (typed messages)
│   ├── slicer-file.ts                mesh upload (REST), download
│   ├── workplate-objects/            the one way an object gets onto a plate
│   ├── model-source/                 object `source_id` → the file it slices from
│   ├── upload-guard.ts               CanDeactivate guard for in-flight uploads
│   ├── viewer-control.ts             camera / framing helpers
│   ├── object-tracker/               per-object UI state
│   ├── print-area/                   build-volume + bed config from server
│   ├── history.ts                    slice history client
│   ├── notifications.ts              toast layer
│   ├── browser-storage.ts            localStorage wrapper
│   ├── logger.service.ts             structured logger (mirrors server logs in console)
│   └── app-theme.ts                  light / dark token switcher
├── schema-form/           generic form renderer driven by JSON Schema
├── models/                shared types (mostly re-exports from generated/)
└── shared/                slicer-only cross-cutting bits — dialog service, icon-button
```

> **Presentational primitives and the design language live in
> [`@coldcrabby/ui`](https://github.com/ColdCrabby/ui), not here.** See
> [Shared UI](#shared-ui-coldcrabbyui) below.

```mermaid
sequenceDiagram
    autonumber
    participant U as User
    participant UI as Angular UI
    participant W as scene-wasm (in browser)
    participant S as Server (WS)

    U->>UI: drop model.stl
    UI->>S: POST /api/upload (multipart, ≤ 500 MB)
    S-->>UI: { ruuid, ofids: [file_uuid] }
    UI->>W: SceneHandle.applyOp(Add { file_id: file_uuid })
    W-->>UI: render buffer (positions, normals, transforms)
    U->>UI: drag / rotate / drop-to-floor
    UI->>W: SceneHandle.applyOp(Translate / Rotate / DropToFloor / …)
    UI->>S: WS Scene { ops }
    U->>UI: click Slice
    UI->>S: WS Slice { request_uuid: ruuid, scene: [{file_id, transform}], settings }
    S-->>UI: WS Progress · PhaseMarker · SliceComplete
    UI-->>U: layered G-code preview + estimates
```

---

## Shared UI (@coldcrabby/ui)

The presentational primitives (button, select, segmented, slider, modal-shell,
…) and the **design language** (theme tokens, base elements, utilities, mixins)
are not defined here — they live in the shared
[`ColdCrabby/ui`](https://github.com/ColdCrabby/ui) repo and are consumed as
**raw source**, so what the slicer renders and what other Cold Crabby apps
render stay identical.

- **Vendored, not published.** `pnpm vendor:ui` clones `ColdCrabby/ui` (tracking
  `main`) into `vendor/coldcrabby-ui/`, which is git-ignored. It also runs on
  `postinstall`, so a fresh `pnpm install` fetches it automatically. To pull the
  latest shared UI, re-run `pnpm vendor:ui`.
- **Imported as `@coldcrabby/ui`.** A tsconfig `paths` entry maps the package to
  `vendor/coldcrabby-ui/src/public-api.ts`; components import primitives from
  `@coldcrabby/ui` directly.
- **Styles via `includePaths`.** `angular.json` adds
  `vendor/coldcrabby-ui/src/styles` to the Sass load path, so
  [src/styles/main.scss](src/styles/main.scss) pulls the shared theme, base,
  utilities, and mixins with bare `@use 'theme/…'` specifiers. Slicer-only
  styles stay local: the **viewport-locking reset** (the app pins the viewport;
  the shared reset scrolls), the global `components/` partials, and the
  `drop-aurora` emit.
- **What stays here.** Slicer-specific UI only: the app shell (`nexus/`), the
  3D viewer, the schema-driven forms, the `fov-cube`, the `dialog` service, and
  the local `icon-button`.

Change a primitive or a token in `ColdCrabby/ui`, open a PR there, and once it
lands, `pnpm vendor:ui` brings it in.

---

## Phones and tablets

The desktop layout assumes horizontal room the slicer does not have on a
handset: a 60px nav rail, a 280px docked settings column and a 380px slice rail
add up to more than a 390px screen _is_. Rather than a second app, the same
shell rearranges itself.

A tablet is the case a single "is it small?" flag gets wrong in both directions.
An iPad has a desktop's width and a phone's input. Treat it as a phone and it
loses a docked settings column it has ample room for; treat it as a desktop —
which is what a lone `handheld()` did — and every panel that floats over the
plate stays open forever, with no cursor to dismiss it, over targets sized for a
mouse. So the shell asks three separate questions and answers them independently.

| Question                               | CSS                | TypeScript          | True on                       |
| -------------------------------------- | ------------------ | ------------------- | ----------------------------- |
| May the layout keep its desktop shape? | `handheld()`       | `isHandheld()`      | phones                        |
| Must chrome over the scene fold away?  | `compact()`        | `isCompact()`       | phones, tablets, ≤1024px      |
| How big must a target be?              | `coarse-pointer()` | `isCoarsePointer()` | phones, tablets, touchscreens |

```mermaid
flowchart LR
    subgraph D["Desktop"]
      direction LR
      dr[nav rail] --- ds[settings column] --- dsc[scene] --- drc[slice rail]
    end
    subgraph T["Tablet"]
      direction LR
      tr[nav rail] --- ts[settings drawer] --- tsc[scene] --- trc[folded rail]
    end
    subgraph P["Phone"]
      direction TB
      pt[toolbar] --- psc[scene] --- prc[slice sheet] --- pn[tab bar]
    end
    D -->|"compact()"| T
    T -->|"handheld()"| P
```

- **One definition of each.** [`styles/_breakpoints.scss`](src/styles/_breakpoints.scss)
  holds all three mixins and [`services/viewport.ts`](src/app/services/viewport.ts)
  holds their three signals. **The query strings are copies of each other and
  must stay identical** — CSS switches the layout, TypeScript switches the
  _controls_ (which ones render at all, whether the settings column may dock),
  and a disagreement shows up as chrome styled for one answer and wired for
  another. Layout itself stays in media queries so a page lays out correctly
  before any script runs.
- **Width and pointer are orthogonal; do not conflate them.** `compact()` is
  about _room_, `coarse-pointer()` about _precision_. A narrow desktop window
  needs the first and not the second; a 12.9" iPad needs the second and not
  obviously the first. Size a target with the pointer, fold a panel with the room.
- **Source order matters more than usual.** A phone matches all three queries,
  so where two blocks set the same property at the same specificity the later
  one wins. Order them **compact → handheld → coarse-pointer**, and prefer
  setting a value in exactly one of them.
- **`html.is-handheld` / `html.is-coarse-pointer` are for the shared components
  only.** [`_handheld.scss`](src/styles/base/_handheld.scss) adapts the layout of
  `@coldcrabby/ui` primitives we do not own (stacking `nexus-field-row`, trimming
  modal gutters); [`_touch.scss`](src/styles/base/_touch.scss) adapts their
  _size_ (34px controls to 44px, an 18px slider thumb to 26px). A component's own
  `:host` block compiles to an attribute selector, which a plain element selector
  loses to; the class buys exactly the specificity needed without `!important`.
  Both are set before first paint by the inline script in `index.html`, and
  `Viewport` keeps them live.

### Folding the chrome over the plate

Everything that hovers over the 3D scene can be folded to a header, because on a
tablet there is no cursor to move away from it and on a phone it is most of the
screen. The pattern is the same in each: a full-width header button with a
chevron, the one readout worth keeping while folded, and a preference that
persists once the user states it.

| Panel            | Header keeps | Default folded |
| ---------------- | ------------ | -------------- |
| G-code inspector | Layer N / M  | `isCompact()`  |
| Object list      | Object count | `isCompact()`  |

Until the user folds or unfolds one, the default is derived; afterwards their
choice is remembered across sessions **and viewports**, because a stated
preference outranks a guess.

**Unfolded, a panel gets the room that is actually there.** The rail card is
bounded by `100dvh` minus the chrome above it — titlebar, safe area, toolbar
inset, its own margins — and the inspector opens to its natural height inside
that, so on any iPad the whole legend and both sliders are visible without
scrolling. A `vh` fraction cannot do this: the same number is too small in
landscape and too generous in portrait. When the room genuinely runs out (a short
desktop window, a phone) the inspector is the thing that shrinks and scrolls —
`min-height: 0` lets flexbox squeeze it, and `flex: none` on the slice row means
the Slice button is never what gets clipped.

What changes on a phone specifically, and why:

| Surface               | Phone form               | Reason                                                                                            |
| --------------------- | ------------------------ | ------------------------------------------------------------------------------------------------- |
| Nav rail              | Bottom tab bar           | 60px of a 390px screen for something used once a session; the bottom edge is what a thumb reaches |
| Print settings        | Drawer + edge tab        | Docked, it leaves ~50px of scene                                                                  |
| Slice rail            | Full-width bottom sheet  | The bottom-right corner is the hardest place on a tall phone to reach                             |
| G-code inspector      | Scrolls inside the sheet | Slice must never be the thing that scrolls away                                                   |
| Settings sections     | Scrollable chip strip    | A 220px column leaves 170px for the settings                                                      |
| Manage pages          | Stacked master–detail    | Two columns need width that is not there                                                          |
| Toasts                | Top of the screen        | The bottom belongs to the sheet and the tab bar                                                   |
| Viewport cube         | Hidden                   | A click-and-drag widget with no touch equivalent, in the corner a phone can least spare           |
| Projection · pipeline | Hidden                   | Eleven pill buttons do not fit; neither is part of getting a model sliced                         |
| Object list           | Collapsible chip         | Expanded it covers a third of the plate it describes                                              |

### The edge tab, and why hover is armed by geometry

Collapsed, the settings drawer leaves a **"Print settings" tab** on the left
edge. Two things about it are deliberate:

- **It hangs just under the toolbar, not at `top: 50%`.** Vertically centred is
  precisely where the model sits, which is the worst place for a permanent
  affordance. The dock nub shares the same anchor, so toggling the drawer changes
  the control's form without moving it. Its hover state is **tone and elevation
  only, never a transform** — the tab's left edge is flush against the nav rail,
  so nudging it sideways tears a gap open between the two.
- **The peek has no backdrop.** The scrim element stays (it is what gives touch a
  tap-outside-to-close gesture) but it is fully transparent: a peek is not a
  modal, and the point of peeking at the settings is to keep watching the plate
  while you change them.
- **The tab does not arm the hover peek — the pointer's own position does.**
  While the peek was armed by the host's own `mouseenter`, hovering the tab
  opened the drawer, which unmounted the tab, which put the pointer on the scene,
  which fired the matching `mouseleave` — open, close, open. Arming now reads
  `clientX` from a document `pointermove` while the panel is collapsed, so there
  is no element to unmount. **Do not "simplify" this back to an invisible edge
  strip**: the strip would need `pointer-events: auto` to receive `mouseenter`,
  the collapsed host is zero-width, and it would therefore lie over the leftmost
  slice of the 3D scene for its whole height — swallowing camera drags,
  click-to-select and, since the sidebar is a _sibling_ of `<main>`, file drops.
  Closing is the same idea: pointer geometry measured against the panel's edge,
  not `mouseleave`, because a panel that mounts, unmounts and slides under a
  stationary pointer emits enter/leave pairs that say nothing about intent.

---

## Touch and pen

A tablet keeps the desktop *layout* — it has the width — but not the desktop
*pointer*. Everything below is about the second half, and lives behind
`Viewport.isCoarsePointer()` rather than `isHandheld()`.

```mermaid
flowchart TB
    down["pointerdown"] --> hit{"raycast hit?"}
    hit -->|object| grab{"selected object,<br/>translate mode,<br/>touch or pen?"}
    hit -->|empty| press2["press: null"]
    grab -->|yes| claim["press: hitId<br/>camera shut out"]
    grab -->|no| press["press: hitId<br/>camera keeps it"]
    claim --> drift{"drift &gt; slop?"}
    press --> drift
    press2 --> drift
    drift -->|no, held 500ms| menu["context menu"]
    drift -->|no, lifted| act["select / clear"]
    drift -->|"yes, and claimed"| drag["slide across bed"]
    drift -->|"yes, otherwise"| orbit["camera orbits"]
```

Everything in that flow is [`scene/selection.ts`](src/app/components/viewer/scene/selection.ts).
The rules worth knowing before editing it:

- **Tap slop is per pointer type** (`TAP_SLOP_PX`): 4px for a mouse, 9 for a pen,
  16 for a finger. One mouse-sized threshold for all three is what made tapping a
  model on an iPad do nothing at all — a fingertip is a ~10mm disc whose reported
  centre wanders as the skin flattens, so most real taps drifted past it and were
  discarded as drags. Any change here is a change to whether touch selection
  works; [`selection.spec.ts`](src/app/components/viewer/scene/selection.spec.ts)
  pins it.
- **A tap resolves on the lift, never the press.** That is what lets a
  mis-aimed press be dragged off to cancel — including in pull-to-floor, where
  the press only *paints* the candidate face (there is no hover on touch to paint
  it earlier) and the lift commits it.
- **Additive selection has no modifier on touch**, so `ViewerControl.additiveSelection`
  is a real mode, toggled from the tool cluster and offered only on
  touch-primary devices with more than one object on the plate. Without it a
  multi-object selection could only be built from the objects list.
- **The long press is the right-click.** iOS never fires `contextmenu` for one,
  so it is recognised from the press itself; the menu is asked for through
  `SceneSelectionHandlers.contextMenu`, and the viewer decides what goes in it.
  **Right-click is driven off the button's press and release, not the
  `contextmenu` event** — Windows raises that after the button comes up and macOS
  the moment it goes down, so only the button's own travel can tell a right
  *click* from the right *drag* that pans the camera.
- **Direct drag is deliberately narrow**: touch or pen, translate mode, and an
  object that is *already* selected. Requiring a prior tap means a model can
  never be shoved across the plate by a stray swipe, and drag-to-orbit stays
  available everywhere else.
- **OrbitControls listens on the same canvas without capture**, and at the target
  the DOM runs capture-flagged listeners before non-capture ones *whatever the
  registration order*. So a `stopPropagation()` from `SceneSelection`'s capture
  handler at `pointerdown` stops the camera starting a rotate at all — which is
  why the drag needs no hand-off once it begins. It is applied only to a press
  the drag will claim; every other press on a model is let through, because
  dragging from a model the user has not picked used to do nothing, making a
  dead zone of most of the scene. A bubble-phase probe in the spec pins both
  directions; break the invariant and the view spins under the dragged object.
- **The lift is never stopped**, whatever the press turned out to be. Since only
  *some* presses are withheld, blocking a `pointerup` strands OrbitControls
  mid-gesture in the ones it *was* let into: its pointer-up handler lives on the
  **document**, so a stop at the canvas reaches it, and without it the pointer
  stays tracked, `state` never returns to `NONE`, and the next button-less move
  orbits the view — on touch, the next single finger reads as a second contact
  and one-finger orbit dies outright. The only surviving stops are on
  `pointermove` inside a live drag, whose `pointerdown` was withheld too, so a
  bubble consumer is absent for the whole gesture rather than half of it.
- **Palm rejection sits above all of it** in
  [`scene/pointer-arbiter.ts`](src/app/components/viewer/scene/pointer-arbiter.ts),
  on the host element in the capture phase, so a resting wrist never reaches any
  of these handlers.

| Surface        | Touch form                     | Reason                                                                    |
| -------------- | ------------------------------ | ------------------------------------------------------------------------- |
| Tap on a model | Selects it                     | The objects list was the only working path                                |
| Multi-select   | Tool-cluster toggle            | There is no ⌘/Ctrl to hold                                                 |
| Long press     | Object / plate context menu    | Puts duplicate, drop, centre, remove where the model is                   |
| Selected model | Drags across the bed           | Three axis arrows inside one contact patch is a coin toss                 |
| Gizmo          | Scaled up (`setSize` 1.4)      | Scaling the helper scales its pickable geometry with it                   |
| Object list    | Starts folded, 44px rows       | It sits on the plate it describes, and cannot be hovered out of the way   |

---

## Monaco Transmit Preview

The **transmit preview panel** is a toggleable side panel that shows, in real time, the exact JSON payloads that the UI would send to the server when a slice job starts.

Toggle it with the **pipeline** (⊞) button — the `filter-list` icon — in the 3D-view toolbar, or press the button again to hide it. The button turns active (highlighted) when the panel is open. The panel sits alongside the 3D viewport and does not obscure the model.

```mermaid
flowchart LR
    engine["SceneEngineService\n(WASM)"]
    slicer["Slicer service\n(settings signal)"]
    shell["NexusSlicingShell\ncomputed signals"]
    panel["Code Editor Panel\n(Monaco, read-only)"]

    engine -- snapshot() --> shell
    slicer -- settings() --> shell
    shell -- snapshotJson\nsliceParamsJson --> panel
```

The panel contains two read-only Monaco editor instances, each updated live as signals change:

| Editor | Content                                           | WebSocket field    |
| ------ | ------------------------------------------------- | ------------------ |
| Top    | Scene snapshot — objects, transforms, world AABBs | `scene` payload    |
| Bottom | Slice settings — layer height, walls, infill, …   | `settings` payload |

`bigint` object IDs are serialised as strings so `JSON.stringify` does not throw.

### `CodeEditorComponent` (`components/code-editor/`)

A thin Angular wrapper around Monaco editor:

- **Lazy-loaded, narrowly, and not until it is looked at** — the editor is composed from Monaco's modular entry points (`editor/editor.api` + `features/register.all`) rather than the package root, and is fetched only once an instance comes within 400 px of the viewport. The root export is `editor.main`, which would register ~90 language grammars and the TypeScript/CSS/HTML language services: a 2.7 MB chunk plus 9.6 MB of web workers, for an app that shows G-code and JSON. **A dynamic `import()` is lazy in the bundle but still runs the moment the component is created** — the printer settings page mounts three editors about six screens below the fold, which used to fetch ~4 MB before the user had scrolled anywhere near them.
- **Inputs**: `content` (string signal), `language` (Monaco language ID, default `'plaintext'`), `readOnly` (boolean).
- **Live updates** — an `effect()` pushes content and readOnly changes into the live editor instance, so Angular signals drive Monaco without re-creating the editor.
- **Resource cleanup** — `DestroyRef.onDestroy` disposes the editor and releases its DOM/worker resources when the component is destroyed.
- **Languages** — `gcode` is ours (a Monarch grammar in `gcode-language.ts`, registered together with the shared `nexus-code` theme, so it loads for every editor). `json` is Monaco's own language service and is fetched only when a JSON editor mounts.
- **Workers** — declared as real module entry points under `workers/`, referenced via `new Worker(new URL(…, import.meta.url))` so the bundler emits base-href-relative assets (a bare specifier only resolved at the site root, which broke sub-path deploys). Only `editor` and `json` are listed: naming a worker in `MonacoEnvironment.getWorker` is what makes the bundler emit it.
- **Options**: dark theme (`vs-dark`), auto-layout, word-wrap on, minimap off, folding on.

### `EditorPanel` service (`services/editor-panel.ts`)

Holds the single `visible: Signal<boolean>` toggle state. Lives in the root injector because the toolbar (toggle button) and the shell (conditional rendering) are in separate component trees. Call `toggle()` to flip it.

---

## Undo / Redo History

Every scene mutation goes through `SceneCommand`, which maintains a snapshot-based undo/redo stack via `SceneHistory`.

```mermaid
sequenceDiagram
    participant Caller as Viewer / Gizmo / Panel
    participant SC as SceneCommand
    participant Engine as SceneEngineService (WASM)
    participant SH as SceneHistory

    Caller->>SC: apply(op)
    SC->>Engine: apply(op) — immediate, no delay
    SC->>SC: capture gestureStart snapshot (first op only)
    SC->>SC: reset 1 s debounce timer

    Note over SC: user pauses ≥ 1 s  (or flush() called)
    SC->>Engine: snapshot()
    SC->>SH: push(before) [first commit only — seeds baseline]
    SC->>SH: push(after)
```

### `SceneCommand` (`services/scene-command/`)

The **only** place where `SceneEngineService.apply` should be called for user-driven mutations. Initialisation paths (`ready()`, `addMesh()`, `resetWithBed()`) still go directly to `SceneEngineService` — they are not undoable.

- `apply(op)` — forwards the op to WASM immediately, then starts/resets a 1-second debounce timer. When the timer fires the gesture is committed to history.
- `flush()` — commit immediately without waiting for the timer. Call on pointer-up / gesture-end events (e.g. drag release in the viewer).

### `SceneHistory` (`services/scene-history/`)

Linear stack of complete `SceneSnapshot` values — no deltas, no partial patches.

| Signal / method  | Description                                   |
| ---------------- | --------------------------------------------- |
| `canUndo`        | `true` when cursor > 0                        |
| `canRedo`        | `true` when cursor < stack tail               |
| `entryCount`     | total snapshots stored                        |
| `push(snapshot)` | append; trims redo branch; caps at 50 entries |
| `undo()`         | step cursor back and restore                  |
| `redo()`         | step cursor forward and restore               |
| `clear()`        | wipe the stack                                |

**Restoration** issues `set_transform` ops for every object in the target snapshot and `remove` ops for objects that no longer exist. Objects that should be re-added but whose mesh bytes are no longer in memory are permanently skipped in the current implementation — re-add support requires a future mesh-byte retention layer.

The baseline snapshot (`s0`) is seeded by `SceneCommand` on the very first gesture commit, so the user can always undo back to the state before any edits.

---

## Keyboard Shortcuts

`KeyboardShortcuts` is eagerly instantiated in `app.config.ts` and adds a single `keydown` listener to `document` for the lifetime of the app.

| Shortcut                  | Action                                     |
| ------------------------- | ------------------------------------------ |
| `Ctrl+Z` (or `⌘Z`)        | Undo                                       |
| `Ctrl+Y` (or `⌘Y`)        | Redo                                       |
| `Ctrl+Shift+Z` (or `⌘⇧Z`) | Redo (alternate — common on macOS / Linux) |

Shortcuts are no-ops when the corresponding history direction is unavailable (guards `canUndo` / `canRedo`). The `keydown` event is consumed with `preventDefault()` only when the shortcut fires, so browser defaults are unaffected otherwise.

---

## Route chunking and navigation feedback

Every screen below `AppShell` is a lazily-loaded chunk, and the initial-bundle
budgets in [angular.json](angular.json) exist to keep it that way. The rules for
what may and may not join the initial download — and why three.js, Monaco and
`marked` each ended up there — are in
[AGENTS.md](../AGENTS.md#bundle-chunking--what-may-sit-in-the-initial-download).
The short version: **anything routed uses `loadComponent`**, and a root-provided
service's imports are initial-bundle imports.

Two pieces keep splitting from turning into waiting:

```mermaid
flowchart LR
    router["Router events"]
    prog["NavigationProgress\n(when to speak)"]
    bar["RouteProgress\nhairline"]
    rails["Nav rail ·\nSettings sub-nav"]
    banner["Update banner"]
    idle["IdleRoutePreload"]

    router --> prog
    prog --> bar
    prog --> rails
    prog -->|chunk fetch failed| banner
    idle -.->|warms chunks so\nmost clicks never wait| router
```

- [`IdleRoutePreload`](src/app/services/route-preload.ts) fetches lazy chunks
  during `requestIdleCallback`, skipping Data Saver and 2G-class connections.
- [`NavigationProgress`](src/app/services/navigation-progress.ts) is the single
  source of truth for "a navigation is taking long enough to mention". It stays
  silent below 120 ms so instant transitions never flash, marks the destination
  rail item as pending, and turns a failed chunk fetch — the signature of a
  redeploy under a long-lived tab — into the existing reload banner via
  `AppVersion.reportStaleAssets()`.

**Because preloading usually wins, the route bar is rarely seen — that is the
intended outcome, not a broken feature.** It appears when a chunk is genuinely
cold: a hard reload straight into a deep link, a slow connection, or a client
where preloading was skipped.

### The boot splash

Route feedback cannot cover the *first* load, because Angular is the thing being
waited for. That gap belongs to
[index.html](src/index.html), which paints a logo, a progress bar and a label
before a single byte of the bundle has run, and tears itself down from
[main.ts](src/main.ts) once the app is on screen.

- **It has to be inline.** A splash component ships inside the bundle it is
  meant to cover, so it could only appear once the wait was already over. Same
  reason its colours are literals rather than design tokens — the stylesheet
  carrying those tokens is part of what is still loading. Keep them in step with
  `--accent` and `--color-bg-primary` by hand.
- **The logo arrives in two stages, and neither is animated.** A ~700-byte WebP
  is embedded in the document as base64, so it paints with the HTML at no
  request cost; `public/splash-logo.webp` (240 px, 22 kB) then cross-fades over
  it. Progressive JPEG, the usual answer for "rough now, sharp later", is not
  available: the logo is RGBA and JPEG has no alpha channel. Neither WebP nor
  AVIF decodes progressively, so the refinement is staged explicitly — which is
  faster anyway, since a progressive format's first pass still costs a round
  trip and an inlined placeholder costs none. Regenerate both with
  `pnpm run splash-logo`; never hand-edit the base64.
- **The progress bar is the only thing that animates.** It is real: the build
  lists every initial chunk in the document as `<link rel="modulepreload">`, and
  a `PerformanceObserver` reports each one as it lands, so the bar tracks actual
  downloads instead of easing along a timer. Downloads map to 0–90 %; the last
  tenth is parse + bootstrap, closed by `__nexusSplashDone()`.
- **Survey the chunk list on every tick, never once at startup.** The build
  appends those `modulepreload` links *after* this inline script, so a single
  survey at parse time finds nothing and the bar never moves — which is exactly
  how it was first written, and what measuring caught.
- **The full-resolution logo is `rel="preload"`ed at high priority**, or it
  queues behind the chunks and arrives after the splash it belongs to has gone.
- Degrades quietly: with no `modulepreload` links (the dev server) or no
  `PerformanceObserver`, the splash still covers the blank page and still
  clears. If the app never boots at all, the label admits it after 30 s rather
  than leaving a bar frozen mid-way.

---

## Generated artifacts

Anything under `src/generated/` is **regenerated, not edited**. Each file maps 1:1 to a Rust type or wasm-pack output, and any drift is treated as a bug in the generator, not in this folder.

| Path                        | Source of truth                                   | Regenerated by                          |
| --------------------------- | ------------------------------------------------- | --------------------------------------- |
| `src/generated/*.d.ts`      | Rust schemas via `slicer-engine gen-schemas`      | `pnpm run gen` (also runs on `install`) |
| `src/generated/scene-wasm/` | `src/scene/wasm.rs` (`cfg(target_arch="wasm32")`) | `make build-wasm` at the repo root      |
| `src/schemas/*.json`        | JSON Schema emitted by the Rust CLI               | `pnpm run gen-schemas`                  |
| `public/splash-logo.webp` + the base64 blob in `src/index.html` | `public/logo_still@3x.png` | `pnpm run splash-logo` at the repo root |

The `postinstall` script in [package.json](package.json) wires this up: cloning the repo and running `pnpm install` (with the WASM bundle already built) is enough to get a working dev environment.

---

## Quick start

```bash
# From the repo root, build the WASM scene engine first
make build-wasm                                  # writes ui/src/generated/scene-wasm/

pnpm install                                     # also runs `pnpm run gen`

# Then, from the repo root, start the engine + dev server together
pnpm run dev                                     # seeded ports: UI 4<seed>, engine 5<seed>
```

`pnpm run dev` ([scripts/dev.mjs](../scripts/dev.mjs)) picks a random seed so
parallel checkouts never fight over a port, and prints the UI URL to open. The
dev server proxies `/api` and `/ws` to the engine
([proxy.conf.mjs](proxy.conf.mjs)), so the app addresses one origin in
development exactly as it does in production.

Running this folder's `pnpm start` on its own is still fine — it serves on the
default `:4213` and proxies to an engine on its default `:5201`.

Reset the generated folder anytime with `pnpm run gen`. If types or schemas look stale after editing Rust, run `pnpm run gen` — never edit `src/generated/` by hand.

---

## Development workflow

| Task                            | Command                                 |
| ------------------------------- | --------------------------------------- |
| Engine + dev server (seeded)    | `pnpm run dev` (repo root)              |
| Dev server with HMR             | `pnpm start`                            |
| Production build                | `pnpm build`                            |
| Watch incremental dev build     | `pnpm watch`                            |
| Unit tests (Vitest, jsdom)      | `pnpm test`                             |
| Regenerate JSON schemas + .d.ts | `pnpm run gen`                          |
| Rebuild the WASM scene engine   | `make build-wasm` (repo root)           |
| Format                          | Prettier (configured in `package.json`) |

The UI follows the project [`.editorconfig`](.editorconfig) and is formatted with Prettier.

---

## Tech stack

- **Angular 21** — standalone components, signals, `provideRouter` with view transitions, zoneless-ready.
- **Monaco Editor** — VS Code's editor component, composed from its modular entry points so only the G-code and JSON languages ship, and deferred until an editor nears the viewport.
- **three.js 0.184** — 3D viewer, custom camera/orbit controls (`viewer-control.ts`), `viewport-cube` orientation widget.
- **Iconoir 7** — icon set.
- **fuse.js 7** — fuzzy search inside settings/history.
- **ngx-markdown 21** — renders Rust READMEs and docs inline where useful.
- **Vitest 4** — fast unit tests via `@angular/build`.
- **wasm-bindgen** (via `scene-wasm`) — typed bridge to the Rust scene engine.

---

## What this UI deliberately does not do

- **No client-side slicing.** The browser only handles scene placement and preview. The slice runs on the server, against the same Rust core.
- **No second source of truth for transforms.** All placement state lives in the WASM `SceneHandle`. The UI reads from it, never duplicates it.
- **No hand-written API types.** If a Rust struct changes, regenerate; do not patch the `.d.ts`.
- **No bundled meshes.** Test fixtures live in `/stls` and [`/tests/fixtures`](../tests/fixtures/) at the repo root.
- **No undo across sessions.** The `SceneHistory` stack is in-memory and is cleared on page reload or navigation. Persistence is a future concern.
- **No undo for mesh uploads / removes.** Re-adding an object requires the original mesh bytes, which are not retained in the history stack. Only transforms are restored on undo.

---

## See also

- [src/scene/README.md](../src/scene/README.md) — the scene engine SSOT this UI sits on top of
- [src/server/README.md](../src/server/README.md) — HTTP + WebSocket protocol
- [src/cli/README.md](../src/cli/README.md) — the same engine, different surface
- [ui/src/styles/README.md](src/styles/README.md) — design tokens and SCSS architecture
- [THEME.md](THEME.md) — colour and spacing system
- [AGENTS.md](../AGENTS.md) — repo-wide conventions and AI-agent guidance
