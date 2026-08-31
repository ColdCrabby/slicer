import {
  Box3,
  BoxGeometry,
  Camera,
  Color,
  Group,
  Mesh,
  Object3D,
  Quaternion,
  Raycaster,
  Scene,
  Vector2,
  Vector3,
  WebGLRenderer,
} from 'three';
import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js';
import type { ObjectMode } from '../../services/viewer-control';

/**
 * High-level delta emitted by {@link GizmoManager} during a drag, suitable
 * for direct dispatch to the WASM scene engine. The manager hides the
 * mechanics of the underlying TransformControls and reports each frame's
 * incremental change against the previous frame.
 */
export type GizmoDelta =
  | { kind: 'translate'; delta: [number, number, number] }
  | { kind: 'rotate'; axis: [number, number, number]; degrees: number }
  | { kind: 'scale'; factors: [number, number, number] };

/** Result of a face-pick raycast for the `pullToFloor` workflow. */
export interface FacePickResult {
  /** `selectableId` of the hit object (string form of the WASM id). */
  objectId: string;
  /** Triangle index inside the hit mesh's geometry. */
  faceIndex: number;
}

/**
 * Tunable colors for the three world axes. World-space only — local space
 * is not supported by this slicer's UI for v1.
 */
const AXIS_COLORS = {
  x: new Color(0xe74c3c),
  y: new Color(0x2ecc71),
  z: new Color(0x3498db),
  active: new Color(0xfff176),
} as const;

/**
 * Constant `size` passed to {@link TransformControls.setSize}. TC handles
 * screen-space-constant sizing internally based on camera distance/FOV;
 * this value is just a unitless multiplier (default 1).
 */
const GIZMO_SCREEN_SIZE = 0.85;

/**
 * Gizmo size where the pointer is a fingertip.
 *
 * TransformControls draws handles a cursor can land on to the pixel; a finger
 * covers roughly a 10mm disc, so at the mouse size the axis arrows sit inside
 * one contact patch and picking the one you meant is a coin toss. Scaling the
 * whole gizmo up scales its pickable geometry with it, which is what actually
 * makes the arrows separable — and it stays proportional, so nothing about the
 * gesture has to change.
 */
const GIZMO_SCREEN_SIZE_TOUCH = 1.4;

/** Whether the device's primary pointer is coarse (a finger, not a cursor). */
function isCoarsePointer(): boolean {
  return (
    typeof window !== 'undefined' &&
    typeof window.matchMedia === 'function' &&
    window.matchMedia('(pointer: coarse)').matches
  );
}

/**
 * Snap step values applied to TransformControls when the user holds
 * Shift during a drag. When Shift is released, snapping is disabled
 * (`null`) so motion is continuous again.
 */
export const GIZMO_SNAP = {
  /** Translation snap in millimetres (world units). */
  translate: 1,
  /** Rotation snap in radians (15°). */
  rotate: (15 * Math.PI) / 180,
  /** Scale snap as a multiplicative step (e.g. 0.1 → 90%, 100%, 110%, …). */
  scale: 0.1,
} as const;

const Z_DOWN = new Vector3(0, 0, -1);

/**
 * Wraps Three.js {@link TransformControls} to provide a mode-aware,
 * always-on-top transform gizmo whose changes are reported as
 * incremental WASM-engine deltas rather than applied directly to a
 * mesh.
 *
 * The gizmo is attached to a hidden "ghost" {@link Group} that is moved
 * to the centroid of the current selection. The ghost is purely a drag
 * surface — its own transform is reset against `lastPose` after each
 * frame so the WASM scene engine remains the single source of truth.
 */
export class GizmoManager {
  private readonly scene: Scene;
  private readonly camera: Camera;
  private readonly renderer: WebGLRenderer;
  /** Ghost target used as the TransformControls "object". Never rendered. */
  private readonly ghost = new Group();
  private readonly translateControls: TransformControls;
  private readonly rotateControls: TransformControls;
  private readonly scaleControls: TransformControls;
  private active: TransformControls | null = null;
  private mode: ObjectMode = 'none';
  /** Last ghost pose seen on `objectChange`; deltas are diffed against this. */
  private readonly lastPosition = new Vector3();
  private readonly lastQuaternion = new Quaternion();
  private readonly lastScale = new Vector3(1, 1, 1);
  /** Anchor (world centroid of selection) the ghost is reset to between drags. */
  private readonly anchor = new Vector3();
  /** True while the user is actively dragging a handle. */
  private dragging = false;
  /**
   * The scale factor emitted in the previous objectChange event for an XYZ
   * uniform-scale drag, used to derive a per-frame incremental ratio from the
   * cumulative total-factor.
   */
  private scaleXyzPrevFactor = 1;
  /** Canvas clientY recorded at the start of an XYZ scale drag. */
  private scaleXyzStartY = 0;
  /** Latest canvas clientY seen during a drag (updated by pointermove). */
  private scaleXyzCurrentY = 0;

  /** Callback fired when dragging starts (so the host can disable OrbitControls). */
  onDragStart: (() => void) | null = null;
  /** Callback fired on each frame's incremental change while dragging. */
  onDelta: ((delta: GizmoDelta) => void) | null = null;
  /** Callback fired when dragging ends (so the host can flush history). */
  onDragEnd: (() => void) | null = null;

  /** Record the pointer Y at the start of any drag so the XYZ scale handler
   *  has a stable origin regardless of TC's internal state. */
  private readonly onScalePointerDown = (e: PointerEvent): void => {
    this.scaleXyzStartY = e.clientY;
    this.scaleXyzCurrentY = e.clientY;
    this.scaleXyzPrevFactor = 1;
  };

  /** Track current pointer Y continuously during drag. */
  private readonly onScalePointerMove = (e: PointerEvent): void => {
    this.scaleXyzCurrentY = e.clientY;
  };

  /** Bound key listeners so we can detach them on dispose. */
  private readonly onKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Shift') {
      // Translate & scale snap on Shift; rotation goes free on Shift.
      this.translateControls.translationSnap = GIZMO_SNAP.translate;
      this.scaleControls.scaleSnap = GIZMO_SNAP.scale;
      this.rotateControls.rotationSnap = null;
    }
  };
  private readonly onKeyUp = (e: KeyboardEvent) => {
    if (e.key === 'Shift') {
      // Release Shift: translate & scale go free; rotation returns to snap.
      this.translateControls.translationSnap = null;
      this.scaleControls.scaleSnap = null;
      this.rotateControls.rotationSnap = GIZMO_SNAP.rotate;
    }
  };

  constructor(scene: Scene, camera: Camera, renderer: WebGLRenderer) {
    this.scene = scene;
    this.camera = camera;
    this.renderer = renderer;

    // The ghost itself has no geometry — it is only a transform target.
    this.ghost.visible = false;
    this.ghost.matrixAutoUpdate = true;
    scene.add(this.ghost);

    this.translateControls = this.makeControls('translate');
    this.rotateControls = this.makeControls('rotate');
    this.scaleControls = this.makeControls('scale');

    this.applyInitialSnapState();
    window.addEventListener('keydown', this.onKeyDown);
    window.addEventListener('keyup', this.onKeyUp);
    this.renderer.domElement.addEventListener('pointerdown', this.onScalePointerDown);
    this.renderer.domElement.addEventListener('pointermove', this.onScalePointerMove);
  }

  /**
   * Snap state is mode-specific: rotate is snapped by default (Shift frees
   * it), while translate and scale are free by default (Shift snaps them).
   * Called once at construction to set the initial state.
   */
  private applyInitialSnapState(): void {
    this.translateControls.translationSnap = null;
    this.rotateControls.rotationSnap = GIZMO_SNAP.rotate;
    this.scaleControls.scaleSnap = null;
  }

  /**
   * Build a TransformControls configured for one mode and wire its event
   * lifecycle into our delta-dispatch pipeline. The helper (the visible
   * gizmo geometry) is added to the scene and hidden until `attach` runs.
   */
  private makeControls(mode: 'translate' | 'rotate' | 'scale'): TransformControls {
    const tc = new TransformControls(this.camera, this.renderer.domElement);
    tc.setMode(mode);
    tc.setSpace('world');
    tc.setColors(AXIS_COLORS.x, AXIS_COLORS.y, AXIS_COLORS.z, AXIS_COLORS.active);
    tc.setSize(isCoarsePointer() ? GIZMO_SCREEN_SIZE_TOUCH : GIZMO_SCREEN_SIZE);
    tc.enabled = false;

    // The visual helper must be added to the scene separately; the
    // TransformControls instance itself is just an event source.
    const helper = tc.getHelper();
    helper.visible = false;
    // Render gizmo on top of everything else so it's never occluded by the
    // model. We walk the helper subtree and disable depth testing on every
    // material — TransformControls uses many small Meshes for its handles.
    helper.traverse((node) => {
      if (node instanceof Mesh) {
        const materials = Array.isArray(node.material) ? node.material : [node.material];
        for (const mat of materials) {
          if (mat) {
            mat.depthTest = false;
            mat.depthWrite = false;
            mat.transparent = true;
          }
        }
        node.renderOrder = 999;
      }
    });

    // Permanently suppress certain handles that TC would re-show every frame:
    //   rotate   — hide the outer 'E' ring (use XYZ axis rings instead)
    //   scale    — hide the planar 'XY'/'YZ'/'XZ' square handles but keep the
    //              central 'XYZ' box for proportional scaling
    //   translate — hide the planar 'XY'/'YZ'/'XZ' squares; replace the central
    //              'XYZ' octahedron with a plain box matching the scale handle
    if (mode === 'rotate' || mode === 'scale' || mode === 'translate') {
      const hiddenNames = mode === 'rotate' ? new Set(['E']) : new Set(['XY', 'YZ', 'XZ']);
      helper.traverse((node) => {
        if (hiddenNames.has(node.name)) {
          Object.defineProperty(node, 'visible', {
            get: () => false,
            set: () => {},
            configurable: true,
          });
        }
      });
    }

    // Replace the translate XYZ octahedron (diamond shape) with a plain box
    // identical to the scale XYZ handle so both look the same.
    if (mode === 'translate') {
      helper.traverse((node) => {
        if (node.name === 'XYZ' && node instanceof Mesh) {
          node.geometry = new BoxGeometry(0.1, 0.1, 0.1);
        }
      });
    }

    this.scene.add(helper);

    tc.addEventListener('mouseDown', () => {
      this.dragging = true;
      this.lastPosition.copy(this.ghost.position);
      this.lastQuaternion.copy(this.ghost.quaternion);
      this.lastScale.copy(this.ghost.scale);
      this.onDragStart?.();
    });
    tc.addEventListener('objectChange', () => {
      this.emitDeltaForMode(mode);
    });
    tc.addEventListener('mouseUp', () => {
      this.dragging = false;
      // Snap the ghost back to the anchor so the next gesture starts from
      // a clean pose — prevents accumulated rotation/scale from biasing
      // subsequent drags.
      this.resetGhost();
      this.onDragEnd?.();
    });

    return tc;
  }

  /**
   * Compute the per-frame delta against {@link lastPosition} /
   * {@link lastQuaternion} / {@link lastScale} for the currently-active
   * mode and forward it to {@link onDelta}. Skips zero-deltas so the
   * scene-command pipeline is not flooded with no-ops.
   */
  private emitDeltaForMode(mode: 'translate' | 'rotate' | 'scale'): void {
    if (!this.onDelta) {
      return;
    }
    if (mode === 'translate') {
      const dx = this.ghost.position.x - this.lastPosition.x;
      const dy = this.ghost.position.y - this.lastPosition.y;
      const dz = this.ghost.position.z - this.lastPosition.z;
      this.lastPosition.copy(this.ghost.position);
      if (dx === 0 && dy === 0 && dz === 0) {
        return;
      }
      this.onDelta({ kind: 'translate', delta: [dx, dy, dz] });
      return;
    }
    if (mode === 'rotate') {
      // delta = current * inverse(previous)
      const prevInv = this.lastQuaternion.clone().invert();
      const deltaQ = this.ghost.quaternion.clone().multiply(prevInv);
      this.lastQuaternion.copy(this.ghost.quaternion);
      // Convert to axis/angle. A quaternion identity (no rotation) reports
      // an angle of 0 — skip those to avoid emitting NaN axes.
      const angle = 2 * Math.acos(Math.min(1, Math.abs(deltaQ.w)));
      if (angle < 1e-6) {
        return;
      }
      const sinHalf = Math.sqrt(Math.max(0, 1 - deltaQ.w * deltaQ.w));
      const sign = deltaQ.w < 0 ? -1 : 1;
      const ax = (deltaQ.x / sinHalf) * sign;
      const ay = (deltaQ.y / sinHalf) * sign;
      const az = (deltaQ.z / sinHalf) * sign;
      const degrees = (angle * 180) / Math.PI;
      this.onDelta({ kind: 'rotate', axis: [ax, ay, az], degrees });
      return;
    }
    // mode === 'scale'
    if (this.scaleControls.axis === 'XYZ') {
      // Use raw canvas pixel coordinates — immune to TC's internal NDC maths.
      // Drag up 150 px = 2× scale; drag down 150 px = 0.5×. Adjust the
      // divisor to taste (larger = slower / less sensitive).
      const dy = this.scaleXyzStartY - this.scaleXyzCurrentY; // positive = drag up
      const totalFactor = Math.pow(2, dy / 150);
      const f = totalFactor / this.scaleXyzPrevFactor;
      this.scaleXyzPrevFactor = totalFactor;
      // Keep lastScale in sync so single-axis drags after an XYZ drag start
      // from a consistent baseline.
      this.lastScale.copy(this.ghost.scale);
      if (Math.abs(f - 1) < 1e-6) {
        return;
      }
      this.onDelta({ kind: 'scale', factors: [f, f, f] });
      return;
    }
    // Single-axis or planar scale — use the ghost scale ratio directly.
    const fx = this.ghost.scale.x / this.lastScale.x;
    const fy = this.ghost.scale.y / this.lastScale.y;
    const fz = this.ghost.scale.z / this.lastScale.z;
    this.lastScale.copy(this.ghost.scale);
    if (fx === 1 && fy === 1 && fz === 1) {
      return;
    }
    this.onDelta({ kind: 'scale', factors: [fx, fy, fz] });
  }

  /**
   * Place the gizmo at `worldCentroid` and show it for the current mode.
   * Called on every selection / mode change. Modes that do not show the
   * native gizmo (`none`, `pullToFloor`) detach instead.
   */
  attach(worldCentroid: Vector3): void {
    this.anchor.copy(worldCentroid);
    this.resetGhost();
    if (this.active) {
      this.active.attach(this.ghost);
      this.active.getHelper().visible = true;
      this.active.enabled = true;
    }
  }

  /** Remove the gizmo from view (no selection, or non-handle mode). */
  detach(): void {
    for (const tc of [this.translateControls, this.rotateControls, this.scaleControls]) {
      tc.detach();
      tc.getHelper().visible = false;
      tc.enabled = false;
    }
  }

  /**
   * Switch the active TransformControls mode. `'none'` and `'pullToFloor'`
   * detach all controls — the ghost stays in the scene but no handles are
   * shown.
   */
  setMode(mode: ObjectMode, worldCentroid: Vector3 | null): void {
    this.mode = mode;
    // Always detach all controls first so only one set of handles is ever
    // visible at a time.
    this.detach();
    if (mode === 'translate') {
      this.active = this.translateControls;
    } else if (mode === 'rotate') {
      this.active = this.rotateControls;
    } else if (mode === 'scale') {
      this.active = this.scaleControls;
    } else {
      this.active = null;
    }
    if (this.active && worldCentroid) {
      this.attach(worldCentroid);
    }
  }

  /** Move the gizmo to a new centroid (e.g. after selection changes). */
  setCentroid(worldCentroid: Vector3 | null): void {
    if (!this.active) {
      return;
    }
    if (!worldCentroid) {
      this.detach();
      return;
    }
    this.attach(worldCentroid);
  }

  /**
   * Per-frame update. TransformControls keeps the gizmo at a constant
   * screen-space size on its own — we just need to keep it attached.
   * `setSize` is a constant unitless multiplier, not a per-frame value.
   */
  update(): void {
    // No-op: TC handles screen-space sizing internally. Kept for API
    // symmetry and in case future versions need per-frame work.
  }

  /** True when the gizmo is currently consuming pointer events. */
  isDragging(): boolean {
    return this.dragging;
  }

  /**
   *window.removeEventListener('keydown', this.onKeyDown);
    window.removeEventListener('keyup', this.onKeyUp);
     True when the gizmo's hover-test passes for the given pointer event,
   * i.e. the cursor is over a transformable handle. Used by the host
   * to suppress the selection raycaster on the same frame.
   */
  isHovering(): boolean {
    return this.active !== null && this.active.axis !== null;
  }

  /**
   * Raycast the active gizmo helper's hit meshes for a pointer position.
   * Used on touch where no prior hover move has set `axis`, so `isHovering()`
   * would incorrectly return false at the moment of touchdown.
   *
   * **Visibility has to be checked by hand.** Three's raycaster tests an
   * object's `layers`, never its `visible` flag, so a hidden helper still
   * reports hits — and TransformControls' pickers are invisible-by-design
   * geometry that is far larger than the handles they stand for. A detached
   * gizmo parks that geometry at the origin, i.e. the middle of the bed, so
   * without these guards *every* tap near the centre of an empty plate was
   * swallowed as "on the gizmo" and no model could be selected by touch or pen
   * at all. A mouse was unaffected, because it reaches `isHovering()` instead.
   */
  hitTest(event: PointerEvent, camera: Camera, renderer: WebGLRenderer): boolean {
    const active = this.active;
    // Detached (nothing selected) or disabled: there are no handles to hit,
    // whatever the picker geometry still lying in the scene says.
    if (!active || !active.enabled) {
      return false;
    }
    const helper = active.getHelper();
    if (!helper.visible) {
      return false;
    }
    const el = renderer.domElement;
    const rect = el.getBoundingClientRect();
    const ndcX = ((event.clientX - rect.left) / Math.max(rect.width, 1)) * 2 - 1;
    const ndcY = -(((event.clientY - rect.top) / Math.max(rect.height, 1)) * 2 - 1);
    const ndc = new Vector2(ndcX, ndcY);
    const rc = new Raycaster();
    rc.setFromCamera(ndc, camera);
    const hits = rc.intersectObject(helper, true);
    // TransformControls also toggles individual handles per mode and axis, so a
    // hit is only real if the thing hit — and everything above it — is showing.
    return hits.some((hit) => isVisibleWithin(hit.object, helper));
  }

  /** Currently active object-manipulation mode. */
  getMode(): ObjectMode {
    return this.mode;
  }

  dispose(): void {
    for (const tc of [this.translateControls, this.rotateControls, this.scaleControls]) {
      tc.detach();
      this.scene.remove(tc.getHelper());
      tc.dispose();
    }
    this.renderer.domElement.removeEventListener('pointerdown', this.onScalePointerDown);
    this.renderer.domElement.removeEventListener('pointermove', this.onScalePointerMove);
    this.scene.remove(this.ghost);
  }

  /** Reset the ghost to the anchor with identity rotation/scale. */
  private resetGhost(): void {
    this.ghost.position.copy(this.anchor);
    this.ghost.quaternion.identity();
    this.ghost.scale.set(1, 1, 1);
    this.ghost.updateMatrixWorld(true);
    this.lastPosition.copy(this.ghost.position);
    this.lastQuaternion.copy(this.ghost.quaternion);
    this.lastScale.copy(this.ghost.scale);
  }
}

/**
 * Whether `object` and every ancestor up to (and including) `root` is visible.
 *
 * Exported for tests. Needed because {@link Raycaster} deliberately ignores
 * `visible` — it filters on `layers` only — so a hit against hidden geometry is
 * reported like any other. Anything deciding "did the user touch this?" from a
 * raycast has to apply the visibility rule itself.
 */
export function isVisibleWithin(object: Object3D, root: Object3D): boolean {
  let node: Object3D | null = object;
  while (node) {
    if (!node.visible) {
      return false;
    }
    if (node === root) {
      return true;
    }
    node = node.parent;
  }
  // Ran past the root without meeting it — not part of this subtree.
  return false;
}

/**
 * Compute the world-space AABB centroid of a set of selectable {@link Object3D}
 * roots. Returns `null` when the selection is empty or every object's
 * bounding box is degenerate.
 */
export function computeSelectionCentroid(objects: readonly Object3D[]): Vector3 | null {
  if (objects.length === 0) {
    return null;
  }
  const aabb = new Box3();
  let any = false;
  const tmp = new Box3();
  for (const obj of objects) {
    obj.updateMatrixWorld(true);
    tmp.makeEmpty();
    tmp.expandByObject(obj);
    if (tmp.isEmpty()) {
      continue;
    }
    if (!any) {
      aabb.copy(tmp);
      any = true;
    } else {
      aabb.union(tmp);
    }
  }
  if (!any) {
    return null;
  }
  return aabb.getCenter(new Vector3());
}

/**
 * Raycast `meshes` for a pointer event and return the WASM `selectableId`
 * + triangle index of the front-most hit, or `null` if the pointer ray
 * missed every mesh. Used by the `pullToFloor` workflow which targets
 * any face on any object regardless of selection state.
 */
export function raycastFace(
  raycaster: Raycaster,
  camera: Camera,
  ndc: Vector2,
  meshes: readonly Object3D[],
): FacePickResult | null {
  raycaster.setFromCamera(ndc, camera);
  const hits = raycaster.intersectObjects(meshes as Object3D[], true);
  for (const hit of hits) {
    if (hit.faceIndex === undefined || hit.faceIndex === null) {
      continue;
    }
    const objectId = findSelectableId(hit.object);
    if (objectId !== null) {
      return { objectId, faceIndex: hit.faceIndex };
    }
  }
  return null;
}

/** Walk up parents until an `userData.selectableId` is found. */
function findSelectableId(obj: Object3D | null): string | null {
  let cur: Object3D | null = obj;
  while (cur) {
    const id = cur.userData?.['selectableId'];
    if (typeof id === 'string') {
      return id;
    }
    cur = cur.parent;
  }
  return null;
}

// Re-exports so the host module doesn't need to import three directly.
export { Z_DOWN };
