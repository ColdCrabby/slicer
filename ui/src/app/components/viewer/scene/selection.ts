import {
  BufferGeometry,
  Color,
  Float32BufferAttribute,
  Material,
  Mesh,
  MeshBasicMaterial,
  Object3D,
  type PerspectiveCamera,
  Plane,
  Raycaster,
  type Scene,
  Vector2,
  Vector3,
  type WebGLRenderer,
} from 'three';
import type { ObjectMode } from '../../../services/viewer-control';
import { computeSelectionCentroid, type GizmoManager, raycastFace } from '../gizmo';
import type { SceneGizmoHandlers, SceneSelectionHandlers } from './types';

/**
 * How far a press may drift and still count as a tap, per pointer type.
 *
 * A mouse click lands on the pixel it started on, so a few pixels is generous.
 * A fingertip is a ~10 mm disc whose reported centre wanders as the skin
 * flattens and the finger rolls off — 4 px of tolerance rejects most genuine
 * taps, which is why tapping a model used to do nothing at all and the objects
 * list was the only way to select anything. A stylus sits between the two:
 * precise, but held at an angle and subject to the hand's own tremor.
 */
const TAP_SLOP_PX: Readonly<Record<string, number>> = {
  mouse: 4,
  pen: 9,
  touch: 16,
};
const DEFAULT_TAP_SLOP_PX = 4;

/**
 * How long a press must be held, without drifting past its slop, to count as a
 * context-menu request. Matches the native iOS/iPadOS long-press so the gesture
 * feels the same here as everywhere else on the device.
 */
const LONG_PRESS_MS = 500;

const SELECTION_EMISSIVE = new Color(0xff8a3d);
const SELECTION_EMISSIVE_INTENSITY = 0.55;

/** Bed normal. Objects sit on Z=0, so a direct drag slides in world XY. */
const DRAG_PLANE_NORMAL = new Vector3(0, 0, 1);

/** A live direct-drag of the selection across the bed. */
interface SelectionDragState {
  /** World point under the pointer on the drag plane, at the last move. */
  last: Vector3;
}

interface SelectionPressState {
  pointerId: number;
  pointerType: string;
  downX: number;
  downY: number;
  /** Drift (px) this press may accumulate and still count as a tap. */
  slopPx: number;
  /** Object under the press, or `null` when it landed on empty space. */
  hitId: string | null;
  additive: boolean;
  /** Set once the press drifts past {@link slopPx}: no longer a tap. */
  moved: boolean;
  /** A long-press already acted on this press, so the lift must not. */
  consumed: boolean;
  longPressTimer: ReturnType<typeof setTimeout> | null;
  /** The event that opened the press, replayed as the context-menu anchor. */
  downEvent: PointerEvent;
  drag: SelectionDragState | null;
}

/**
 * Manages selectable object registration, emissive highlight, raycasting,
 * pull-to-floor face-picking, and all pointer event plumbing for object
 * selection and gizmo hand-off.
 */
export class SceneSelection {
  selectionHandlers: SceneSelectionHandlers | null = null;
  gizmoHandlers: SceneGizmoHandlers | null = null;

  private currentObjectMode: ObjectMode = 'none';
  private readonly selectables = new Map<string, Object3D>();
  private currentSelectedIds: ReadonlySet<string> = new Set();

  get selectedIds(): ReadonlySet<string> {
    return this.currentSelectedIds;
  }
  private readonly originalEmissive = new Map<Mesh, { color: Color; intensity: number }[]>();
  private readonly raycaster = new Raycaster();
  private readonly ndcScratch = new Vector2();
  private pressState: SelectionPressState | null = null;

  /**
   * Makes a plain tap toggle its object in and out of the selection, the way
   * ⌘/Ctrl-click does with a mouse. Touch has no modifier key, so without this
   * a multi-object selection could only be built from the objects list.
   */
  private additiveSelection = false;

  /**
   * Whether dragging an already-selected object slides the selection across the
   * bed. Enabled for touch and pen, where the gizmo's thin arrows are a poor
   * target for a fingertip; a mouse keeps drag-to-orbit and uses the gizmo.
   */
  private directDragEnabled = false;

  /** Plane the direct drag slides along — horizontal, through the selection. */
  private readonly dragPlane = new Plane(new Vector3(0, 0, 1), 0);
  private readonly dragPointScratch = new Vector3();

  /**
   * A held right mouse button, tracked so a right *click* can open the context
   * menu while a right *drag* still pans the camera.
   *
   * The `contextmenu` event cannot tell the two apart on its own — Windows
   * raises it after the button is released, macOS and Linux the moment it goes
   * down — so the menu is driven off the button's own press and release
   * instead, which reads the same everywhere.
   */
  private rightPress: {
    pointerId: number;
    downEvent: PointerEvent;
    moved: boolean;
  } | null = null;

  // Face-highlight overlay for pull-to-floor mode
  private readonly faceHighlight: Mesh = (() => {
    const geo = new BufferGeometry();
    geo.setAttribute('position', new Float32BufferAttribute(new Float32Array(9), 3));
    const mat = new MeshBasicMaterial({
      color: 0x2ecc71,
      transparent: true,
      opacity: 0.55,
      depthTest: false,
      depthWrite: false,
      side: 2, // DoubleSide
    });
    const m = new Mesh(geo, mat);
    m.renderOrder = 998;
    m.visible = false;
    m.matrixAutoUpdate = false;
    return m;
  })();
  private readonly faceTriScratchA = new Vector3();
  private readonly faceTriScratchB = new Vector3();
  private readonly faceTriScratchC = new Vector3();
  private faceHighlightCache: { meshUuid: string; groupId: number } | null = null;
  private pendingHighlightEvent: PointerEvent | null = null;
  private highlightRafHandle = 0;

  constructor(
    private readonly scene: Scene,
    private readonly camera: PerspectiveCamera,
    private readonly renderer: WebGLRenderer,
    private readonly gizmo: GizmoManager,
  ) {
    scene.add(this.faceHighlight);
    this.install();
  }

  install(): void {
    const el = this.renderer.domElement;
    el.addEventListener('pointerdown', this.onPointerDown, { capture: true });
    el.addEventListener('pointermove', this.onPointerMove, { capture: true });
    el.addEventListener('pointerup', this.onPointerUp, { capture: true });
    el.addEventListener('pointercancel', this.onPointerCancel, { capture: true });
    el.addEventListener('contextmenu', this.onContextMenu);
  }

  uninstall(): void {
    const el = this.renderer.domElement;
    el.removeEventListener('pointerdown', this.onPointerDown, { capture: true });
    el.removeEventListener('pointermove', this.onPointerMove, { capture: true });
    el.removeEventListener('pointerup', this.onPointerUp, { capture: true });
    el.removeEventListener('pointercancel', this.onPointerCancel, { capture: true });
    el.removeEventListener('contextmenu', this.onContextMenu);
  }

  /** See {@link additiveSelection}. */
  setAdditiveSelection(on: boolean): void {
    this.additiveSelection = on;
  }

  /** See {@link directDragEnabled}. */
  setDirectDragEnabled(on: boolean): void {
    this.directDragEnabled = on;
  }

  register(id: string, object: Object3D): void {
    object.userData['selectableId'] = id;
    this.selectables.set(id, object);
  }

  unregister(id: string): void {
    const obj = this.selectables.get(id);
    if (!obj) {
      return;
    }
    if (this.currentSelectedIds.has(id)) {
      this.applyHighlight(obj, false);
    }
    delete obj.userData['selectableId'];
    this.selectables.delete(id);
    // Drop the id from the selection too. Leaving it behind keeps the gizmo
    // attached to an object that no longer exists, and the next drag then
    // dispatches a Translate against a dead id — which throws and aborts the
    // move for every other selected object.
    if (this.currentSelectedIds.has(id)) {
      const remaining = new Set(this.currentSelectedIds);
      remaining.delete(id);
      this.currentSelectedIds = remaining;
      this.gizmo.setCentroid(this.computeSelectionCentroid());
    }
  }

  clearAll(): void {
    for (const obj of this.selectables.values()) {
      this.applyHighlight(obj, false);
      delete obj.userData['selectableId'];
    }
    this.selectables.clear();
    this.currentSelectedIds = new Set();
    this.originalEmissive.clear();
  }

  setSelectedIds(ids: ReadonlySet<string>): void {
    for (const id of this.currentSelectedIds) {
      if (!ids.has(id)) {
        const obj = this.selectables.get(id);
        if (obj) {
          this.applyHighlight(obj, false);
        }
      }
    }
    for (const id of ids) {
      if (!this.currentSelectedIds.has(id)) {
        const obj = this.selectables.get(id);
        if (obj) {
          this.applyHighlight(obj, true);
        }
      }
    }
    this.currentSelectedIds = ids;
    this.gizmo.setCentroid(this.computeSelectionCentroid());
  }

  getSelectedIds(): ReadonlySet<string> {
    return this.currentSelectedIds;
  }

  /**
   * Temporarily remove (or restore) the emissive selection highlight on the
   * currently-selected objects, without changing the selection itself. Used by
   * off-screen thumbnail capture so the selection glow never bleeds into the
   * rendered image.
   */
  setHighlightVisible(visible: boolean): void {
    for (const id of this.currentSelectedIds) {
      const obj = this.selectables.get(id);
      if (obj) {
        this.applyHighlight(obj, visible);
      }
    }
  }

  setObjectTransform(
    id: string,
    transform: {
      position: { x: number; y: number; z: number };
      rotation: { x: number; y: number; z: number };
      scale: { x: number; y: number; z: number };
    },
  ): void {
    const obj = this.selectables.get(id);
    if (!obj) {
      return;
    }
    const { position, rotation, scale } = transform;
    if (
      obj.position.x !== position.x ||
      obj.position.y !== position.y ||
      obj.position.z !== position.z
    ) {
      obj.position.set(position.x, position.y, position.z);
    }
    if (
      obj.rotation.x !== rotation.x ||
      obj.rotation.y !== rotation.y ||
      obj.rotation.z !== rotation.z
    ) {
      obj.rotation.set(rotation.x, rotation.y, rotation.z);
    }
    if (obj.scale.x !== scale.x || obj.scale.y !== scale.y || obj.scale.z !== scale.z) {
      obj.scale.set(scale.x, scale.y, scale.z);
    }
  }

  computeSelectionCentroid(): Vector3 | null {
    if (this.currentSelectedIds.size === 0) {
      return null;
    }
    const objects: Object3D[] = [];
    for (const id of this.currentSelectedIds) {
      const obj = this.selectables.get(id);
      if (obj) {
        objects.push(obj);
      }
    }
    return computeSelectionCentroid(objects);
  }

  cancelActiveDrag(): void {
    this.abandonPress();
  }

  setObjectMode(mode: ObjectMode): void {
    this.currentObjectMode = mode;
    this.gizmo.setMode(mode, this.computeSelectionCentroid());
    if (mode !== 'pullToFloor') {
      this.hideFaceHighlight();
    }
  }

  dispose(): void {
    this.uninstall();
    // A pending long-press must not fire into a torn-down scene.
    this.endPress();
    this.rightPress = null;
    if (this.highlightRafHandle !== 0) {
      cancelAnimationFrame(this.highlightRafHandle);
      this.highlightRafHandle = 0;
    }
    this.faceHighlight.geometry.dispose();
    (this.faceHighlight.material as Material).dispose();
    this.scene.remove(this.faceHighlight);
  }

  // -------------------------------------------------------------------------
  // Pointer event handlers
  // -------------------------------------------------------------------------

  private onPointerDown = (event: PointerEvent): void => {
    if (event.button === 2) {
      // Right button: a click opens the menu, a drag pans. Which it was is only
      // known on release, so just remember where it started.
      this.rightPress = { pointerId: event.pointerId, downEvent: event, moved: false };
      return;
    }
    if (event.button !== 0 || !this.selectionHandlers) {
      return;
    }
    // On touch — and on a stylus tap without a preceding hover move — there is
    // no prior pointermove, so axis is null and isHovering() returns false even
    // when the pointer is directly over a gizmo handle. hitTest does a live
    // raycast so touch and pen can still pass through to TransformControls.
    const directHitPointer = event.pointerType === 'touch' || event.pointerType === 'pen';
    const onGizmo =
      this.gizmo.isHovering() ||
      this.gizmo.isDragging() ||
      (directHitPointer && this.gizmo.hitTest(event, this.camera, this.renderer));
    if (onGizmo) {
      return;
    }

    // A second contact belongs to a camera gesture (pinch / pan / roll), never
    // to a selection — with two exceptions, in this order.
    const live = this.pressState;
    if (live !== null && live.pointerId !== event.pointerId) {
      // A drag is an explicit, committed gesture: the model being moved keeps
      // the contact that started it and extra fingers are ignored.
      if (live.drag) {
        return;
      }
      // A pen outranks a resting hand. On a hover-less iPad the palm can land a
      // beat before the tip and be admitted (the arbiter's size heuristic is
      // only armed once a pen has been seen *recently*), so without this the
      // palm's press would hold the slot and every Pencil tap after it would be
      // thrown away until the hand lifted.
      const penOverridesTouch = event.pointerType === 'pen' && live.pointerType === 'touch';
      this.abandonPress();
      if (!penOverridesTouch) {
        return;
      }
    }

    const hitId = this.selectables.size > 0 ? this.raycastSelectable(event) : null;
    this.pressState = {
      pointerId: event.pointerId,
      pointerType: event.pointerType,
      downX: event.clientX,
      downY: event.clientY,
      slopPx: TAP_SLOP_PX[event.pointerType] ?? DEFAULT_TAP_SLOP_PX,
      hitId,
      additive: this.additiveSelection || event.ctrlKey || event.metaKey || event.shiftKey,
      moved: false,
      consumed: false,
      longPressTimer: null,
      downEvent: event,
      drag: null,
    };

    // With no hover on touch, the face about to be pulled to the floor is
    // invisible until something paints it — so paint it on contact and commit
    // on the lift, which also lets a mis-aimed press be dragged off to cancel.
    if (this.currentObjectMode === 'pullToFloor') {
      this.updateFaceHighlight(event);
    }

    // Pull-to-floor is a picking mode: a held press there is someone lining up
    // a face, not asking for a menu.
    //
    // No trailing-click guard is needed, unlike the generic `ContextMenuTrigger`
    // directive: a press on a model already cancels its compatibility mouse
    // events via `preventDefault` below, and the menu opens offset from the
    // pointer, so the lift's click cannot land on an item. A blanket swallow
    // here would sit armed and eat the user's *next* tap instead.
    if (
      this.selectionHandlers.contextMenu &&
      event.pointerType !== 'mouse' &&
      this.currentObjectMode !== 'pullToFloor'
    ) {
      const press = this.pressState;
      press.longPressTimer = setTimeout(() => {
        press.longPressTimer = null;
        press.consumed = true;
        this.fireContextMenu(press.hitId, press.downEvent);
      }, LONG_PRESS_MS);
    }

    if (hitId !== null) {
      event.preventDefault();
      // Keep the camera out of a gesture that is about to move this object —
      // and only then.
      //
      // `stopPropagation` in the capture phase is what decides this: at the
      // target the DOM runs capture-flagged listeners before non-capture ones
      // whatever the registration order, and OrbitControls listens on this same
      // canvas without capture. So stopping here means it never starts a
      // rotate, and no hand-off is needed once the drag begins.
      //
      // Every other press on a model is let through, so a drag that starts on
      // something the user has not picked still orbits the view — dragging from
      // a model used to do nothing at all, which on a touch screen turns most
      // of the scene into a dead zone.
      if (this.claimsDirectDrag(hitId, event.pointerType)) {
        event.stopPropagation();
      }
    }
  };

  private onPointerMove = (event: PointerEvent): void => {
    const right = this.rightPress;
    if (right && event.pointerId === right.pointerId && !right.moved) {
      const drift = Math.hypot(
        event.clientX - right.downEvent.clientX,
        event.clientY - right.downEvent.clientY,
      );
      if (drift >= (TAP_SLOP_PX[event.pointerType] ?? DEFAULT_TAP_SLOP_PX)) {
        right.moved = true;
      }
    }
    if (this.currentObjectMode === 'pullToFloor') {
      this.pendingHighlightEvent = event;
      if (this.highlightRafHandle === 0) {
        this.highlightRafHandle = requestAnimationFrame(this.flushFaceHighlight);
      }
    }
    const ps = this.pressState;
    if (!ps || event.pointerId !== ps.pointerId || !this.selectionHandlers) {
      return;
    }
    if (ps.drag) {
      this.advanceDrag(ps, event);
      // Safe to stop: a drag only exists for a press whose `pointerdown` was
      // withheld, so every bubble-phase consumer of this pointer is absent for
      // the whole gesture rather than half of it.
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    if (ps.moved || ps.consumed) {
      return;
    }
    const drift = Math.hypot(event.clientX - ps.downX, event.clientY - ps.downY);
    if (drift < ps.slopPx) {
      return;
    }
    ps.moved = true;
    this.clearLongPress(ps);
    if (this.beginDrag(ps, event)) {
      event.preventDefault();
      event.stopPropagation();
    }
  };

  private onPointerUp = (event: PointerEvent): void => {
    const right = this.rightPress;
    if (right && event.pointerId === right.pointerId) {
      this.rightPress = null;
      // A right click that never travelled: the menu, at the press position.
      if (!right.moved && this.selectionHandlers?.contextMenu) {
        this.fireContextMenu(this.raycastSelectable(right.downEvent), right.downEvent);
        return;
      }
    }
    const ps = this.pressState;
    if (!ps || event.pointerId !== ps.pointerId) {
      return;
    }
    const { hitId, additive, moved, consumed, drag } = ps;
    this.endPress();

    // The lift is never stopped, whatever the press turned out to be.
    //
    // Only *some* presses are withheld from the camera (see `pointerdown`), and
    // OrbitControls' own pointer-up handler sits on the document — an ancestor,
    // so a stop here reaches it. Blocking the lift of a press it *was* let into
    // would strand it mid-gesture: a phantom tracked pointer, `state` never
    // returning to `NONE`, and a subsequent button-less move rotating the view.
    // For a press it never saw, letting the lift through costs nothing.
    if (drag) {
      this.gizmoHandlers?.end();
      event.preventDefault();
      return;
    }
    // The long-press already acted on this press; the lift only ends it.
    if (moved || consumed) {
      return;
    }

    if (this.currentObjectMode === 'pullToFloor') {
      const hit = hitId !== null ? this.pickFace(event) : null;
      if (hit) {
        this.hideFaceHighlight();
        this.gizmoHandlers?.facePicked(hit.objectId, hit.faceIndex);
        event.preventDefault();
      } else if (this.currentSelectedIds.size > 0) {
        this.selectionHandlers?.clearSelection();
      }
      return;
    }

    if (hitId === null) {
      if (this.currentSelectedIds.size > 0) {
        this.selectionHandlers?.clearSelection();
      }
      return;
    }
    this.selectionHandlers?.select(hitId, additive);
    event.preventDefault();
  };

  private onPointerCancel = (event: PointerEvent): void => {
    if (this.rightPress?.pointerId === event.pointerId) {
      this.rightPress = null;
    }
    this.hideFaceHighlight();
    const ps = this.pressState;
    if (!ps || event.pointerId !== ps.pointerId) {
      return;
    }
    if (ps.drag) {
      this.gizmoHandlers?.end();
    }
    this.endPress();
  };

  /**
   * The OS menu never appears over the viewport — a right click is either a pan
   * (handled by the camera) or our own menu, opened from the button's release
   * so the two can be told apart on every platform.
   */
  private onContextMenu = (event: MouseEvent): void => {
    event.preventDefault();
  };

  // -------------------------------------------------------------------------
  // Press lifecycle
  // -------------------------------------------------------------------------

  private endPress(): void {
    const ps = this.pressState;
    if (!ps) {
      return;
    }
    this.clearLongPress(ps);
    ps.drag = null;
    this.pressState = null;
  }

  /**
   * Give up the press without resolving it — the camera has claimed the
   * gesture, or a second contact arrived.
   *
   * A live direct drag has already moved the objects, so it is committed
   * rather than dropped: leaving the scene-command batch open would strand
   * the move outside the undo history.
   */
  private abandonPress(): void {
    const dragging = Boolean(this.pressState?.drag);
    this.endPress();
    if (dragging) {
      this.gizmoHandlers?.end();
    }
  }

  private clearLongPress(ps: SelectionPressState): void {
    if (ps.longPressTimer !== null) {
      clearTimeout(ps.longPressTimer);
      ps.longPressTimer = null;
    }
  }

  private fireContextMenu(hitId: string | null, event: PointerEvent | MouseEvent): void {
    this.selectionHandlers?.contextMenu?.(hitId, event);
  }

  // -------------------------------------------------------------------------
  // Direct drag — slide the selection across the bed
  // -------------------------------------------------------------------------

  /**
   * Whether a press here would become a direct drag rather than a camera move.
   *
   * Deliberately narrow: only a touch or pen contact, only in translate mode,
   * and only on an object that is *already* selected. Requiring a prior tap
   * means a model can never be shoved across the plate by a stray swipe, and it
   * keeps drag-to-orbit available everywhere else on the scene.
   *
   * Asked at `pointerdown`, before it is known whether the press will travel,
   * because that is when the camera has to be shut out (see there).
   */
  private claimsDirectDrag(hitId: string | null, pointerType: string): boolean {
    return (
      this.directDragEnabled &&
      this.gizmoHandlers !== null &&
      this.currentObjectMode === 'translate' &&
      (pointerType === 'touch' || pointerType === 'pen') &&
      hitId !== null &&
      this.currentSelectedIds.has(hitId)
    );
  }

  /**
   * Take a drifting press over as a move of the whole selection.
   *
   * The camera was already shut out of this contact at `pointerdown`, so there
   * is nothing to wrestle it away from here — see {@link claimsDirectDrag}.
   *
   * @returns whether the drag started.
   */
  private beginDrag(ps: SelectionPressState, event: PointerEvent): boolean {
    if (!this.claimsDirectDrag(ps.hitId, ps.pointerType)) {
      return false;
    }
    const centroid = this.computeSelectionCentroid();
    if (!centroid) {
      return false;
    }
    // A horizontal plane through the selection: objects slide over the bed,
    // never up off it. Depth stays the gizmo's (and gravity's) business.
    this.dragPlane.set(DRAG_PLANE_NORMAL, -centroid.z);
    const start = this.pointOnDragPlane(event);
    if (!start) {
      return false;
    }
    // Follow the finger even if it leaves the canvas — dragging a model to the
    // edge of the bed puts it under the toolbar or the objects list, and
    // without capture the drag would stall there until the finger came back.
    try {
      this.renderer.domElement.setPointerCapture(event.pointerId);
    } catch {
      // The pointer may already be gone; the drag simply stays canvas-bound.
    }
    ps.drag = { last: start.clone() };
    return true;
  }

  private advanceDrag(ps: SelectionPressState, event: PointerEvent): void {
    const drag = ps.drag;
    if (!drag) {
      return;
    }
    const point = this.pointOnDragPlane(event);
    if (!point) {
      return;
    }
    const dx = point.x - drag.last.x;
    const dy = point.y - drag.last.y;
    drag.last.copy(point);
    if (dx === 0 && dy === 0) {
      return;
    }
    this.gizmoHandlers?.delta([...this.currentSelectedIds], {
      kind: 'translate',
      delta: [dx, dy, 0],
    });
  }

  /** Where the pointer ray meets the drag plane, or null looking edge-on. */
  private pointOnDragPlane(event: PointerEvent): Vector3 | null {
    this.raycaster.setFromCamera(this.toNdc(event, this.ndcScratch), this.camera);
    return this.raycaster.ray.intersectPlane(this.dragPlane, this.dragPointScratch);
  }

  // -------------------------------------------------------------------------
  // Face picking (pull-to-floor)
  // -------------------------------------------------------------------------

  private pickFace(event: PointerEvent): { objectId: string; faceIndex: number } | null {
    const ndc = this.toNdc(event, this.ndcScratch);
    const targets = Array.from(this.selectables.values());
    if (targets.length === 0) {
      return null;
    }
    return raycastFace(this.raycaster, this.camera, ndc, targets);
  }

  private updateFaceHighlight(event: PointerEvent): void {
    const ndc = this.toNdc(event, this.ndcScratch);
    const targets = Array.from(this.selectables.values());
    if (targets.length === 0) {
      this.hideFaceHighlight();
      return;
    }
    this.raycaster.setFromCamera(ndc, this.camera);
    const hits = this.raycaster.intersectObjects(targets, true);

    for (const hit of hits) {
      const mesh = hit.object;
      const face = hit.face;
      if (!face || !(mesh instanceof Mesh) || !mesh.geometry) {
        continue;
      }
      const posAttr = mesh.geometry.getAttribute('position');
      if (!posAttr) {
        continue;
      }

      const faceGroups: Uint32Array | undefined = mesh.userData['faceGroups'];
      const hitFaceIdx = hit.faceIndex ?? 0;
      const targetGroup =
        faceGroups && faceGroups.length > hitFaceIdx ? faceGroups[hitFaceIdx] : -1;

      const cache = this.faceHighlightCache;
      if (
        cache !== null &&
        cache.meshUuid === mesh.uuid &&
        cache.groupId === targetGroup &&
        this.faceHighlight.visible
      ) {
        return;
      }

      let faceIndices: number[];
      if (faceGroups && targetGroup >= 0) {
        faceIndices = [];
        for (let i = 0; i < faceGroups.length; i++) {
          if (faceGroups[i] === targetGroup) {
            faceIndices.push(i);
          }
        }
      } else {
        faceIndices = [hitFaceIdx];
      }

      const triCount = faceIndices.length;
      const posArr = new Float32Array(triCount * 9);

      const va0 = this.faceTriScratchA.fromBufferAttribute(posAttr, face.a);
      const vb0 = this.faceTriScratchB.fromBufferAttribute(posAttr, face.b);
      const vc0 = this.faceTriScratchC.fromBufferAttribute(posAttr, face.c);
      mesh.localToWorld(va0);
      mesh.localToWorld(vb0);
      mesh.localToWorld(vc0);
      const nx = (vb0.y - va0.y) * (vc0.z - va0.z) - (vb0.z - va0.z) * (vc0.y - va0.y);
      const ny = (vb0.z - va0.z) * (vc0.x - va0.x) - (vb0.x - va0.x) * (vc0.z - va0.z);
      const nz = (vb0.x - va0.x) * (vc0.y - va0.y) - (vb0.y - va0.y) * (vc0.x - va0.x);
      const nlen = Math.hypot(nx, ny, nz) || 1;
      const lift = 0.02;
      const lx = (nx / nlen) * lift;
      const ly = (ny / nlen) * lift;
      const lz = (nz / nlen) * lift;

      const indexAttr = mesh.geometry.getIndex();

      for (let t = 0; t < triCount; t++) {
        const fi = faceIndices[t];
        let ia: number, ib: number, ic: number;
        if (indexAttr) {
          ia = indexAttr.getX(fi * 3);
          ib = indexAttr.getX(fi * 3 + 1);
          ic = indexAttr.getX(fi * 3 + 2);
        } else {
          ia = fi * 3;
          ib = fi * 3 + 1;
          ic = fi * 3 + 2;
        }
        const va = this.faceTriScratchA.fromBufferAttribute(posAttr, ia);
        const vb = this.faceTriScratchB.fromBufferAttribute(posAttr, ib);
        const vc = this.faceTriScratchC.fromBufferAttribute(posAttr, ic);
        mesh.localToWorld(va);
        mesh.localToWorld(vb);
        mesh.localToWorld(vc);
        const base = t * 9;
        posArr[base] = va.x + lx;
        posArr[base + 1] = va.y + ly;
        posArr[base + 2] = va.z + lz;
        posArr[base + 3] = vb.x + lx;
        posArr[base + 4] = vb.y + ly;
        posArr[base + 5] = vb.z + lz;
        posArr[base + 6] = vc.x + lx;
        posArr[base + 7] = vc.y + ly;
        posArr[base + 8] = vc.z + lz;
      }

      const existing = this.faceHighlight.geometry.getAttribute('position');
      if (
        existing instanceof Float32BufferAttribute &&
        (existing.array as Float32Array).length === posArr.length
      ) {
        (existing.array as Float32Array).set(posArr);
        existing.needsUpdate = true;
      } else {
        this.faceHighlight.geometry.setAttribute('position', new Float32BufferAttribute(posArr, 3));
      }
      this.faceHighlight.geometry.deleteAttribute('index');
      this.faceHighlight.geometry.computeBoundingSphere();
      this.faceHighlight.visible = true;
      this.faceHighlightCache = { meshUuid: mesh.uuid, groupId: targetGroup };
      return;
    }
    this.hideFaceHighlight();
  }

  private hideFaceHighlight(): void {
    this.faceHighlight.visible = false;
    this.faceHighlightCache = null;
    if (this.highlightRafHandle !== 0) {
      cancelAnimationFrame(this.highlightRafHandle);
      this.highlightRafHandle = 0;
    }
    this.pendingHighlightEvent = null;
  }

  private flushFaceHighlight = (): void => {
    this.highlightRafHandle = 0;
    const ev = this.pendingHighlightEvent;
    this.pendingHighlightEvent = null;
    if (ev !== null && this.currentObjectMode === 'pullToFloor') {
      this.updateFaceHighlight(ev);
    }
  };

  // -------------------------------------------------------------------------
  // Raycast helpers
  // -------------------------------------------------------------------------

  private raycastSelectable(event: MouseEvent): string | null {
    const ndc = this.toNdc(event, this.ndcScratch);
    this.raycaster.setFromCamera(ndc, this.camera);
    const targets = Array.from(this.selectables.values());
    if (targets.length === 0) {
      return null;
    }
    const hits = this.raycaster.intersectObjects(targets, true);
    if (hits.length === 0) {
      return null;
    }
    return this.findSelectableId(hits[0].object);
  }

  private findSelectableId(obj: Object3D | null): string | null {
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

  private toNdc(event: MouseEvent, out: Vector2): Vector2 {
    const rect = this.renderer.domElement.getBoundingClientRect();
    const x = ((event.clientX - rect.left) / Math.max(rect.width, 1)) * 2 - 1;
    const y = -(((event.clientY - rect.top) / Math.max(rect.height, 1)) * 2 - 1);
    return out.set(x, y);
  }

  // -------------------------------------------------------------------------
  // Emissive highlight
  // -------------------------------------------------------------------------

  private applyHighlight(root: Object3D, on: boolean): void {
    root.traverse((node) => {
      if (!(node instanceof Mesh)) {
        return;
      }
      const materials = Array.isArray(node.material) ? node.material : [node.material];
      if (on) {
        const snapshot: { color: Color; intensity: number }[] = [];
        for (const mat of materials) {
          const m = mat as Material & {
            emissive?: Color;
            emissiveIntensity?: number;
          };
          if (!m.emissive) {
            snapshot.push({ color: new Color(0, 0, 0), intensity: 0 });
            continue;
          }
          snapshot.push({
            color: m.emissive.clone(),
            intensity: m.emissiveIntensity ?? 1,
          });
          m.emissive.copy(SELECTION_EMISSIVE);
          if ('emissiveIntensity' in m) {
            m.emissiveIntensity = SELECTION_EMISSIVE_INTENSITY;
          }
        }
        this.originalEmissive.set(node, snapshot);
      } else {
        const snapshot = this.originalEmissive.get(node);
        if (!snapshot) {
          return;
        }
        for (let i = 0; i < materials.length; i++) {
          const m = materials[i] as Material & {
            emissive?: Color;
            emissiveIntensity?: number;
          };
          const orig = snapshot[i];
          if (!m.emissive || !orig) {
            continue;
          }
          m.emissive.copy(orig.color);
          if ('emissiveIntensity' in m) {
            m.emissiveIntensity = orig.intensity;
          }
        }
        this.originalEmissive.delete(node);
      }
    });
  }
}
