import {
  BufferGeometry,
  Color,
  Float32BufferAttribute,
  Material,
  Mesh,
  MeshBasicMaterial,
  Object3D,
  type PerspectiveCamera,
  Raycaster,
  type Scene,
  Vector2,
  Vector3,
  type WebGLRenderer,
} from 'three';
import type { ObjectMode } from '../../../services/viewer-control';
import { computeSelectionCentroid, type GizmoManager, raycastFace } from '../gizmo';
import type { SceneGizmoHandlers, SceneSelectionHandlers } from './types';

const SELECTION_DRAG_THRESHOLD_PX = 4;
const SELECTION_EMISSIVE = new Color(0xff8a3d);
const SELECTION_EMISSIVE_INTENSITY = 0.55;

interface SelectionPressState {
  pointerId: number;
  downX: number;
  downY: number;
  hitId: string;
  additive: boolean;
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
  private emptyPressState: {
    pointerId: number;
    downX: number;
    downY: number;
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
  }

  uninstall(): void {
    const el = this.renderer.domElement;
    el.removeEventListener('pointerdown', this.onPointerDown, { capture: true });
    el.removeEventListener('pointermove', this.onPointerMove, { capture: true });
    el.removeEventListener('pointerup', this.onPointerUp, { capture: true });
    el.removeEventListener('pointercancel', this.onPointerCancel, { capture: true });
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
    this.pressState = null;
    this.emptyPressState = null;
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
    if (this.currentObjectMode === 'pullToFloor') {
      const hit = this.pickFace(event);
      if (hit) {
        event.preventDefault();
        event.stopPropagation();
        this.hideFaceHighlight();
        this.gizmoHandlers?.facePicked(hit.objectId, hit.faceIndex);
      } else if (this.currentSelectedIds.size > 0) {
        this.emptyPressState = {
          pointerId: event.pointerId,
          downX: event.clientX,
          downY: event.clientY,
        };
      }
      return;
    }
    // Orbit mode is fixed; clicks always attempt object selection.
    if (this.selectables.size === 0) {
      return;
    }
    const hitId = this.raycastSelectable(event);
    if (hitId === null) {
      if (this.currentSelectedIds.size > 0) {
        this.emptyPressState = {
          pointerId: event.pointerId,
          downX: event.clientX,
          downY: event.clientY,
        };
      }
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    this.pressState = {
      pointerId: event.pointerId,
      downX: event.clientX,
      downY: event.clientY,
      hitId,
      additive: event.ctrlKey || event.metaKey || event.shiftKey,
    };
  };

  private onPointerMove = (event: PointerEvent): void => {
    if (this.currentObjectMode === 'pullToFloor') {
      this.pendingHighlightEvent = event;
      if (this.highlightRafHandle === 0) {
        this.highlightRafHandle = requestAnimationFrame(this.flushFaceHighlight);
      }
    }
    const eps = this.emptyPressState;
    if (eps && event.pointerId === eps.pointerId) {
      const dxPx = event.clientX - eps.downX;
      const dyPx = event.clientY - eps.downY;
      if (Math.hypot(dxPx, dyPx) >= SELECTION_DRAG_THRESHOLD_PX) {
        this.emptyPressState = null;
      }
    }
    const ps = this.pressState;
    if (!ps || event.pointerId !== ps.pointerId || !this.selectionHandlers) {
      return;
    }
    const dxPx = event.clientX - ps.downX;
    const dyPx = event.clientY - ps.downY;
    if (Math.hypot(dxPx, dyPx) >= SELECTION_DRAG_THRESHOLD_PX) {
      this.pressState = null;
    }
  };

  private onPointerUp = (event: PointerEvent): void => {
    const eps = this.emptyPressState;
    if (eps && event.pointerId === eps.pointerId) {
      this.emptyPressState = null;
      const dxPx = event.clientX - eps.downX;
      const dyPx = event.clientY - eps.downY;
      if (Math.hypot(dxPx, dyPx) < SELECTION_DRAG_THRESHOLD_PX) {
        this.selectionHandlers?.clearSelection();
      }
    }
    const ps = this.pressState;
    if (!ps || event.pointerId !== ps.pointerId) {
      return;
    }
    this.pressState = null;
    this.selectionHandlers?.select(ps.hitId, ps.additive);
    event.preventDefault();
    event.stopPropagation();
  };

  private onPointerCancel = (event: PointerEvent): void => {
    this.hideFaceHighlight();
    if (this.emptyPressState && event.pointerId === this.emptyPressState.pointerId) {
      this.emptyPressState = null;
    }
    const ps = this.pressState;
    if (!ps || event.pointerId !== ps.pointerId) {
      return;
    }
    this.cancelActiveDrag();
  };

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

  private raycastSelectable(event: PointerEvent): string | null {
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

  private toNdc(event: PointerEvent, out: Vector2): Vector2 {
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
