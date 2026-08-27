# Viewer — 3D Canvas, Gizmos, and Object Manipulation

The viewer is the single entry point for 3D visualisation. It owns the
Three.js scene, the WebGL render loop, camera controls, and — introduced in
this PR — a full suite of interactive object-manipulation gizmos.

> _Every gesture the user makes becomes a `SceneOp`. The renderer reads the
> result back. It never invents transforms of its own._

---

## Why gizmos live here

Scene placement is owned by the Rust scene engine (compiled to WASM). The
viewer's job is to translate pointer events into the right `SceneOp` and then
reflect the updated state back into Three.js.

Before this PR only orbit/pan/zoom were interactive. Object placement required
clicking toolbar buttons ("center on bed", "drop to floor"). The gizmo layer
adds direct on-canvas manipulation without breaking the SSOT contract: every
drag still ends with a `Rotate`, `Translate`, or `Scale` op dispatched to the
WASM engine; Three.js receives the resulting matrix and mirrors it — nothing
more.

---

## Object-manipulation modes

The `ObjectMode` union defined in `viewer-control.ts` drives what happens when
the user interacts with a selected mesh:

| Mode          | What it shows                                   | What it does on interaction              |
| ------------- | ----------------------------------------------- | ---------------------------------------- |
| `none`        | No gizmo (default)                              | Clicks select / deselect objects         |
| `translate`   | Three-axis / three-plane translation handles    | Emits `Translate` ops per frame          |
| `rotate`      | Three arc rotation handles                      | Emits `Rotate` ops per frame             |
| `scale`       | Three-axis scale handles (no planar handles)    | Emits `Scale` ops per frame              |
| `pullToFloor` | Face-highlight cursor; no handles on the canvas | Single face-pick → `AlignFaceToFloor` op |

The toolbar (`3d-view-toolbar`) exposes one button per mode in a radio group
that writes to `ViewerControl.objectMode`. The viewer reacts via an Angular
`effect()` and calls `ViewerScene.setObjectMode()`.

```mermaid
flowchart LR
    Toolbar -->|objectMode signal| VC[ViewerControl]
    VC -->|effect| VS[ViewerScene.setObjectMode]
    VS --> GM[GizmoManager.setMode]
    GM --> TC[TransformControls x3]
```

---

## GizmoManager

`GizmoManager` (`gizmo.ts`) wraps Three.js
[`TransformControls`](https://threejs.org/docs/#examples/en/controls/TransformControls)
and hides all its mechanical details behind a clean delta-stream interface.

```mermaid
classDiagram
    class GizmoManager {
        +onDragStart: () => void
        +onDelta: (GizmoDelta) => void
        +onDragEnd: () => void
        +attach(worldCentroid: Vector3)
        +detach()
        +setMode(mode, centroid)
        +setCentroid(centroid)
        +isDragging() bool
        +isHovering() bool
        +dispose()
    }
    class GizmoDelta {
        <<union>>
        translate: delta [x,y,z]
        rotate: axis [x,y,z] + degrees
        scale: factors [x,y,z]
    }
    GizmoManager --> GizmoDelta : emits via onDelta
```

### Ghost object

`GizmoManager` attaches `TransformControls` to an invisible `Group` (the
"ghost") instead of directly to a scene mesh. The ghost lives at the world
centroid of the current selection and acts purely as a drag surface. At the end
of each frame the ghost is reset to its anchor with identity rotation and unit
scale — so the WASM engine is always the authoritative record of where things
actually are.

### Incremental deltas

`TransformControls` reports absolute transforms of its target object. The
manager converts those to per-frame incremental deltas that map directly to
WASM ops:

- **translate** → position difference from last frame → `Translate { delta }`
- **rotate** → quaternion difference → axis-angle decomposition → `Rotate { axis, degrees }`
- **scale** → ratio of current to last scale → `Scale { factors }`

Zero-magnitude deltas are filtered before dispatch so the WASM pipeline is not
flooded with no-ops.

### Shift-key snapping

While **Shift** is held, each TransformControls instance snaps to a fixed step
defined in `GIZMO_SNAP`:

| Mode      | Snap step  |
| --------- | ---------- |
| translate | 1 mm       |
| rotate    | 15°        |
| scale     | 0.1 (±10%) |

When Shift is released, the snap reverts to continuous free motion (`null`).

### Always-on-top rendering

Gizmo handles are rendered on top of the model so they are never occluded.
Every `Mesh` node in the `TransformControls` helper tree has its material
configured with `depthTest: false`, `depthWrite: false`, `transparent: true`,
and `renderOrder: 999`. This is applied once in `makeControls()` — from that
point `TransformControls` reuses the same materials.

---

## Pull-to-floor — face picking

`pullToFloor` is a one-shot mode: the user clicks any face on any object and
that face is aligned to the build plate floor (Z = 0) via an
`AlignFaceToFloor` op. The mode exits automatically to `'none'` after the
pick.

### How face picking works

```mermaid
sequenceDiagram
    participant U as User (click)
    participant VS as ViewerScene
    participant GM as GizmoManager
    participant W as WASM engine

    U->>VS: pointerdown (pullToFloor mode)
    VS->>GM: raycastFace(raycaster, camera, ndc, meshes)
    GM-->>VS: FacePickResult { objectId, faceIndex }
    VS->>W: AlignFaceToFloor { id, face_index }
    W-->>VS: updated matrix
    VS->>VS: setObjectMode('none')
```

`raycastFace()` runs a Three.js raycaster against all scene meshes and returns
the `selectableId` (the string form of the WASM object id stored in
`userData.selectableId`) and the triangle index of the nearest hit.

### Face-group highlighting

When the cursor enters `pullToFloor` mode, the viewer asks the WASM engine for
coplanar face groups (`SceneEngineService.getFaceGroups`). As the cursor moves,
`raycastFace` identifies the hovered triangle and the viewer highlights all
faces in the same coplanar group — giving the user a clear preview of which
flat face will be aligned to the floor.

The highlight is applied as a per-vertex color buffer attribute on a cloned
`BufferGeometry`. The original geometry is restored when the mode exits.

---

## Coplanar face groups (Rust side)

`src/mesh/analysis.rs::compute_coplanar_groups` is the Rust function that
powers the face-pick highlight. See [src/mesh/README.md](../../../../../src/mesh/README.md)
for the full algorithm description.

The WASM bridge method `SceneHandle.getFaceGroups(id, angleThresholdDeg)` calls
it and returns a `Uint32Array` of group ids (one per triangle). The
`SceneEngineService.getFaceGroups()` wrapper logs the call timing and hands the
array to the viewer.

---

## Interaction priority

When multiple input consumers are active at the same time (orbit, selection
raycaster, gizmo), the viewer applies a clear priority order:

0. **Palm rejection (stylus in use)** — [`PointerArbiter`](scene/pointer-arbiter.ts)
   listens in the _capture_ phase on the canvas host (an ancestor of the WebGL
   canvas), so it runs before every other consumer. While an Apple Pencil /
   stylus is down, hovering, or was active within the grace window, it swallows
   `pointerType === 'touch'` events — the hand and wrist resting on the glass —
   so the palm never orbits, pinches, or selects. Genuine finger gestures are
   untouched whenever no pen is involved.
1. **Gizmo dragging in progress** — gizmo owns the pointer; OrbitControls
   and the selection raycaster are both suppressed.
2. **Gizmo hovering** (cursor over a handle, not yet dragging) — the selection
   raycaster is suppressed on this frame so a click registers on the handle,
   not on an underlying mesh.
3. **pullToFloor mode** — selection raycaster is disabled; pointer is entirely
   dedicated to face picking.
4. **Normal mode** — the selection raycaster runs; OrbitControls handles any
   gesture that misses a selectable object.

### Viewport-cube auto-ortho

Clicking a viewport-cube face/edge/corner snaps the camera to that view **and
flattens the projection to orthographic** — the CAD convention that a snapped
view is dimension-true. The temporary ortho is held until the user **pans or
zooms** the main viewport, at which point the projection reverts to whatever the
toolbar preset was (normally perspective). **Rotating** the view or interacting
with the cube again keeps ortho.

| Action                                    | Auto-ortho       |
| ----------------------------------------- | ---------------- |
| Cube face/edge/corner snap                | engage (→ ortho) |
| Rotate (1-finger / left-drag / swipe)     | keep             |
| Cube drag-orbit / roll / re-snap          | keep             |
| **Pan** (2-finger / right-drag / ⌥-swipe) | **revert**       |
| **Zoom** (pinch / wheel / autoscroll)     | **revert**       |
| Toolbar view toggle / home reset          | cancel (manual)  |

This lives entirely in [`SceneCamera`](scene/camera.ts) as a projection override
(`autoOrtho`) — it deliberately does **not** touch the toolbar `view` signal, so
there is no signal-ordering race between the snap animation and a view toggle.
Engaging animates to the snapped direction at ~1° FOV with an apparent-size-
preserving distance; reverting is an **instant** apparent-size-preserving FOV
swap (`notifyUserPanOrZoom`) so it never fights the live pan/zoom gesture that
triggered it. The pan/zoom trigger is emitted only from the genuine pan/zoom
input sites in [`SceneControls`](scene/controls.ts) (`setRevertGestureSink`) —
rotate and cube-driven moves never emit it.

### Pen-priority palm rejection ("wrist detection")

On an iPad the hand resting on the glass while drawing with an Apple Pencil
fires `touch` pointer events for the palm and wrist. Unfiltered, they drive the
camera — OrbitControls' single-touch rotate spins the view and two palm
contacts read as a pinch — so the model lurches while the user works with the
pencil. [`PointerArbiter`](scene/pointer-arbiter.ts) vetoes those contacts.

```mermaid
flowchart TD
    E[pointer event on host<br/>capture phase] --> P{pointerType?}
    P -->|pen| T[track pen: penActive + penEverUsed<br/>pass through]
    P -->|touch| C{palm?}
    P -->|mouse| A[pass through]
    C -->|pen active, in grace,<br/>or palm-sized after pen use| S[stopImmediatePropagation<br/>swallow]
    C -->|otherwise| A
    T --> D[OrbitControls / selection / gizmo]
    A --> D
```

A touch is judged palm at its `pointerdown` (`isPalmTouch`, unit-tested) when a
pen is active — down, hovering, or lifted within `PEN_GRACE_MS` — or, once a pen
has been seen this session, when its contact patch is palm-sized
(`PALM_CONTACT_MIN_PX`), which catches the palm that lands just before the tip on
iPads without pencil hover. Pure-touch users are never affected: the contact-size
path is gated behind "a pen has been seen", and the pen-active path only fires
while a pen is in use. The user can turn the whole behaviour off from
**Settings → General → Controls → Palm rejection** (persisted; default on).

### Hand-aware inspector tooltip placement

The G-code inspector tooltip (extrusion width/height/speed on hover in the
scalar views) is anchored to a virtual element at the pointer. A fixed
below-right placement is ideal with a mouse but lands the readout **directly
under the palm** of a right-handed pen user. `preferredHoverPlacement`
([hover-placement.ts](hover-placement.ts), unit-tested) picks the side per input:

| Pointer | Placement                                                    |
| ------- | ------------------------------------------------------------ |
| mouse   | `right-start` — the familiar below-right desktop behaviour.  |
| touch   | `top` — the finger and hand occlude below, so float above.   |
| pen     | opposite the tilt (hand) direction; `top` when near-upright. |

The elegant part is the pen case: `PointerEvent.tiltX`/`tiltY` point from the tip
toward the barrel — i.e. toward the hand — so the tooltip floats to the _opposite_
side. Because tilt reveals which way the pen leans, this adapts to left- vs.
right-handed users with **no setting to configure**. Floating UI's flip/shift
still keep it on-screen, so this only chooses the _preferred_ side. The
`viewer.ts` effect re-anchors when the preferred side changes (input swap or a
tilt that crosses an axis).

---

## Anatomy

The viewer is split into focused files so each concern stays under ~300 lines.

```
viewer/
├── viewer.ts                  Angular component — effects wiring, WASM ↔ Three bridge
├── hover-placement.ts         preferredHoverPlacement — hand-aware inspector-tooltip side (pen tilt)
├── scene/                     ViewerScene and all Three.js sub-systems
│   ├── index.ts               ViewerScene — owns renderer, render loop, delegates to sub-modules
│   ├── camera.ts              SceneCamera — animations, view presets, fit-to-content, near/far
│   ├── controls.ts            SceneControls — orbit inertia, multi-touch (pinch/pan/roll), autoscroll zoom
│   ├── grid.ts                SceneGrid — adaptive build-plate grid with cross-fade and fade-on-graze
│   ├── pointer-arbiter.ts     PointerArbiter — pen-priority palm rejection (capture-phase touch veto)
│   ├── selection.ts           SceneSelection — selectable registry, emissive highlight, raycasting, face-pick
│   ├── types.ts               Shared public types (SceneSelectionHandlers, SceneGizmoHandlers, ViewerView, …)
│   └── utils.ts               disposeObject — recursive Three.js geometry/material cleanup
├── gizmo.ts                   GizmoManager, computeSelectionCentroid, raycastFace
├── gcode-orchestrator.ts      GcodeOrchestrator — owns layer groups; Three.js visibility only (no geometry)
├── gcode-layer-renderer.ts    buildLayerGroup, showLayerRange, applySegmentProgress, applyHiddenRoles
└── index.ts                   Public re-exports
```

### ViewerScene sub-module responsibilities

| File                       | Class            | Owns                                                                                        |
| -------------------------- | ---------------- | ------------------------------------------------------------------------------------------- |
| `scene/camera.ts`          | `SceneCamera`    | `PerspectiveCamera` pose, view animations, `fitToContent`                                   |
| `scene/controls.ts`        | `SceneControls`  | `OrbitControls` config, orbit inertia, touch gestures, autoscroll zoom                      |
| `scene/grid.ts`            | `SceneGrid`      | Bed grid `LineSegments`, adaptive spacing, CSS theme integration                            |
| `scene/pointer-arbiter.ts` | `PointerArbiter` | Pen-priority palm rejection — capture-phase touch veto while a stylus is in use             |
| `scene/selection.ts`       | `SceneSelection` | Selectable `Map`, emissive highlight, pointer event plumbing, face-pick overlay             |
| `scene/index.ts`           | `ViewerScene`    | Three.js primitives (`Scene`, `WebGLRenderer`, `OrbitControls`), `contentRoot`, render loop |

### G-code layer architecture

All G-code geometry is built exclusively inside `GcodeOrchestrator.buildFromHandle()` by
calling the WASM-side `GcodeSource.getLayer()`. Three.js receives finished `Float32Array`
buffers and is responsible only for showing/hiding layer groups and scrubbing segment
draw-ranges. No geometry is constructed in TypeScript.

```mermaid
flowchart LR
    WASM[GcodeSource\nWASM handle] -->|getLayer| GO[GcodeOrchestrator\nbuildFromHandle]
    GO -->|LineSegments| CR[contentRoot\nThree.js scene]
    GPS[GcodePreviewService\nsignals] -->|showRange\napplyProgress\napplyHiddenRoles| GO
```

---

## What this module deliberately does _not_ do

- **No scene-state ownership.** Transforms are not stored here. The WASM
  `SceneHandle` is the only truth; Three.js matrices are a read-only mirror of
  it.
- **No G-code geometry construction.** `GcodeOrchestrator` never builds
  `BufferGeometry` itself — it only routes the WASM-emitted `Float32Array`
  buffers into Three.js `LineSegments` via `gcode-layer-renderer`.
- **No multi-object gizmo.** When multiple objects are selected, the gizmo
  appears at their collective world centroid but each object receives
  independent `Rotate`/`Translate`/`Scale` ops with the same delta. A unified
  pivot is not implemented for v1.
- **No undo stack.** `onDragEnd` is the hook point for a history layer; the
  viewer itself discards the delta stream when the drag ends.

---

## See also

- [scene/index.ts](scene/index.ts) — `ViewerScene`, `SceneSelectionHandlers`, `SceneGizmoHandlers`
- [scene/camera.ts](scene/camera.ts) — `SceneCamera`
- [scene/controls.ts](scene/controls.ts) — `SceneControls`
- [scene/grid.ts](scene/grid.ts) — `SceneGrid`
- [scene/pointer-arbiter.ts](scene/pointer-arbiter.ts) — `PointerArbiter`, `isPalmTouch`, `PEN_GRACE_MS`, `PALM_CONTACT_MIN_PX`
- [scene/selection.ts](scene/selection.ts) — `SceneSelection`
- [gizmo.ts](gizmo.ts) — `GizmoManager`, `GizmoDelta`, `FacePickResult`, `raycastFace`, `computeSelectionCentroid`
- [hover-placement.ts](hover-placement.ts) — `preferredHoverPlacement`, `HoverPointerInfo`
- [gcode-orchestrator.ts](gcode-orchestrator.ts) — `GcodeOrchestrator`
- [gcode-layer-renderer.ts](gcode-layer-renderer.ts) — layer builder and visibility helpers
- [viewer.ts](viewer.ts) — Angular component wiring
- [../../services/viewer-control.ts](../../services/viewer-control.ts) — `ObjectMode`, `ViewerControl` signal store
- [../../services/scene-engine.service.ts](../../services/scene-engine.service.ts) — `SceneEngineService`, `getFaceGroups`
- [../../../../src/mesh/README.md](../../../../../src/mesh/README.md) — coplanar group algorithm
- [../../../../src/scene/README.md](../../../../../src/scene/README.md) — scene engine SSOT contract
