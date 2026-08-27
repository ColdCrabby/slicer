import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  type ElementRef,
  afterNextRender,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import {
  BufferGeometry,
  CanvasTexture,
  Color,
  ConeGeometry,
  CylinderGeometry,
  DoubleSide,
  Float32BufferAttribute,
  Group,
  LinearFilter,
  Matrix4,
  Mesh,
  MeshBasicMaterial,
  PerspectiveCamera,
  PlaneGeometry,
  Raycaster,
  Scene,
  Sprite,
  SpriteMaterial,
  Vector2,
  Vector3,
  WebGLRenderer,
} from 'three';
import { ViewerControl } from '../../services/viewer-control';

/**
 * The scene is Z-up. The six cardinal look directions (target → camera) map to
 * named faces:
 *  - +X = RIGHT, -X = LEFT
 *  - +Y = BACK,  -Y = FRONT
 *  - +Z = TOP,   -Z = BOTTOM
 * Every snapped view keeps the camera up as world Z (0,0,1) — the main camera
 * (see `SceneCamera.animateToDirection`) nudges pure Top/Bottom looks a hair
 * off the pole rather than flipping up sideways, so resuming an orbit from any
 * face/edge/corner feels identical.
 */
const WORLD_UP = new Vector3(0, 0, 1);

/** The six cardinal axes — reused for faces, arrow snapping, and face-on detection. */
const CARDINALS: readonly Vector3[] = [
  new Vector3(1, 0, 0),
  new Vector3(-1, 0, 0),
  new Vector3(0, 1, 0),
  new Vector3(0, -1, 0),
  new Vector3(0, 0, 1),
  new Vector3(0, 0, -1),
];

/**
 * A view counts as "looking straight at a face" (which reveals the four rotate
 * arrows) when the camera direction is within this angle of a cardinal axis.
 */
const FACE_ON_COS = Math.cos((9 * Math.PI) / 180);

/** Clickable region of the cube. */
type ZoneKind = 'face' | 'edge' | 'corner';

/** Per-mesh metadata attached to every clickable zone via `userData`. */
interface ZoneUserData {
  kind: ZoneKind;
  /** Stable identity used to track hover across theme rebuilds. */
  key: string;
  /** Unit look direction (target → camera) this zone snaps the camera to. */
  direction: Vector3;
  /** Face label (faces only). */
  label?: string;
  /** Base / hover tint for bevel facets (edges + corners). */
  baseColor?: Color;
  hoverColor?: Color;
  baseOpacity?: number;
  hoverOpacity?: number;
}

/** Static definition of one labelled face tile. */
interface FaceDef {
  label: string;
  normal: Vector3;
  /** World direction that should appear "up" on the face texture. */
  faceUp: Vector3;
}

const FACE_DEFS: readonly FaceDef[] = [
  { label: 'RIGHT', normal: new Vector3(1, 0, 0), faceUp: new Vector3(0, 0, 1) },
  { label: 'LEFT', normal: new Vector3(-1, 0, 0), faceUp: new Vector3(0, 0, 1) },
  { label: 'BACK', normal: new Vector3(0, 1, 0), faceUp: new Vector3(0, 0, 1) },
  { label: 'FRONT', normal: new Vector3(0, -1, 0), faceUp: new Vector3(0, 0, 1) },
  { label: 'TOP', normal: new Vector3(0, 0, 1), faceUp: new Vector3(0, 1, 0) },
  { label: 'BOTTOM', normal: new Vector3(0, 0, -1), faceUp: new Vector3(0, -1, 0) },
];

const CUBE_SIZE = 1;
const HALF = CUBE_SIZE / 2;
// Chamfer inset: how far each flat face tile is pulled back from the raw cube
// edge. The reclaimed border becomes the bevelled edge + corner facets. Kept
// thin so the faces dominate and the edge/corner hotspots read as slim bevels
// rather than large panels.
const CHAMFER = 0.1;
// Half-extent of a face tile (the flat, labelled square).
const FACE_HALF = HALF - CHAMFER;

// Half-extent of the cube's bounding sphere — guarantees the cube fits no
// matter how it is rotated (corner distance from center is sqrt(3)/2 * size).
const CUBE_HALF_EXTENT = (CUBE_SIZE * Math.sqrt(3)) / 2;
// Padding factor around the cube inside the orthographic frustum. Wider than
// strictly needed for the cube alone so the dimensional-guide axes (which
// run along the three cube edges meeting at the -X/-Y/-Z corner) and their
// X/Y/Z end labels stay fully on-screen at every camera orientation.
const FRUSTUM_PADDING = 1.6;

// RGB axes gizmo — colour convention matches the main scene's
// `buildAxesGizmo` (X = red, Y = green, Z = blue) so the orientation cube
// reads identically to the build-plate gizmo in the main viewer.
const AXIS_COLOR_X = 0xff3344;
const AXIS_COLOR_Y = 0x33dd55;
const AXIS_COLOR_Z = 0x4488ff;
// The gizmo lives at the cube's -X/-Y/-Z corner (visually "bottom-left-back")
// and its three coloured shafts run **along** the three cube edges that meet
// at that corner. The shafts therefore double as dimensional guides: each
// edge is annotated with the world-axis (X/Y/Z) it represents, with an arrow
// head + label at the +end of every edge so the user can read the build-volume
// orientation at a glance. Each shaft is offset slightly outboard of its edge
// (perpendicular to its own axis) so it sits just outside the cube faces and
// doesn't z-fight with the textured face beneath it.
const AXIS_EDGE_OFFSET = 0.04;
const AXIS_LENGTH = CUBE_SIZE;
const AXIS_SHAFT_RADIUS = 0.018;
const AXIS_HEAD_LENGTH = 0.14;
const AXIS_HEAD_RADIUS = 0.05;
// Small perpendicular tick mark at the origin end of each axis — mimics the
// end caps on architectural dimension lines, making it obvious that the
// coloured shaft represents the *length* of the cube edge along that axis.
const AXIS_TICK_LENGTH = 0.09;
const AXIS_TICK_RADIUS = 0.01;
// Distance from the tip of the arrow head to the centre of the X/Y/Z label
// sprite, expressed in world units of the cube scene.
const AXIS_LABEL_OFFSET = 0.12;
const AXIS_LABEL_SIZE = 0.26;

// Faces and bevels are fully opaque so the cube reads as a solid graphite
// widget — you can't see the back faces or the interior through the front,
// and only the outboard RGB axis shafts remain visible past the silhouette.
const BEVEL_OPACITY = 1;
const BEVEL_HOVER_OPACITY = 1;
// Distance from the camera to the cube. Arbitrary for an orthographic camera
// — only direction matters — but kept large enough to stay well inside the
// near/far range.
const CUBE_DISTANCE = 5;
// Drag-to-orbit sensitivity (radians per pixel).
const DRAG_SENSITIVITY = 0.01;
// Pointer-move distance (in pixels) below which a pointer-up still counts as
// a click rather than the end of a drag.
const CLICK_DRAG_THRESHOLD = 4;

/**
 * Traditional ViewCube-style orientation widget. Renders a chamfered, labelled
 * cube (six faces, twelve edge bevels, eight corner bevels) whose orientation
 * mirrors the main viewer camera. Click a face / edge / corner to snap the main
 * camera to that orthographic / isometric view; click-and-drag the cube to orbit
 * freely. When the camera looks straight at a face, four arrows appear that step
 * the view 90° to the neighbouring faces.
 */
@Component({
  selector: 'nexus-viewport-cube',
  standalone: true,
  template: `
    <canvas #canvas class="cube-canvas"></canvas>
    @if (faceOn()) {
      <div class="roll-buttons">
        <button
          type="button"
          class="roll-btn roll-ccw"
          aria-label="Roll view counter-clockwise"
          (click)="roll('ccw')"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path
              d="M15.55 5.55 11 1v3.07C7.06 4.56 4 7.92 4 12s3.05 7.44 7 7.93v-2.02c-2.84-.48-5-2.94-5-5.91s2.16-5.43 5-5.91V10l4.55-4.45zM19.93 11c-.17-1.39-.72-2.73-1.62-3.89l-1.42 1.42c.54.75.88 1.6 1.02 2.47h2.02zM13 17.9v2.02c1.39-.17 2.74-.71 3.9-1.61l-1.44-1.44c-.75.54-1.59.89-2.46 1.03zm3.89-2.42 1.42 1.41c.9-1.16 1.45-2.5 1.62-3.89h-2.02c-.14.87-.48 1.72-1.02 2.48z"
            />
          </svg>
        </button>
        <button
          type="button"
          class="roll-btn roll-cw"
          aria-label="Roll view clockwise"
          (click)="roll('cw')"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path
              d="M15.55 5.55 11 1v3.07C7.06 4.56 4 7.92 4 12s3.05 7.44 7 7.93v-2.02c-2.84-.48-5-2.94-5-5.91s2.16-5.43 5-5.91V10l4.55-4.45zM19.93 11c-.17-1.39-.72-2.73-1.62-3.89l-1.42 1.42c.54.75.88 1.6 1.02 2.47h2.02zM13 17.9v2.02c1.39-.17 2.74-.71 3.9-1.61l-1.44-1.44c-.75.54-1.59.89-2.46 1.03zm3.89-2.42 1.42 1.41c.9-1.16 1.45-2.5 1.62-3.89h-2.02c-.14.87-.48 1.72-1.02 2.48z"
            />
          </svg>
        </button>
      </div>
    }
  `,
  styles: [
    `
      :host {
        display: block;
        position: relative;
        width: 96px;
        height: 96px;
        background: transparent;
        pointer-events: auto;
        user-select: none;
        touch-action: none;
        overflow: hidden;
      }
      .cube-canvas {
        display: block;
        width: 100%;
        height: 100%;
        background: transparent;
        cursor: grab;
      }
      .cube-canvas.is-dragging {
        cursor: grabbing;
      }
      .roll-buttons {
        position: absolute;
        inset: 0;
        pointer-events: none;
      }
      .roll-btn {
        position: absolute;
        top: 4px;
        display: grid;
        place-items: center;
        width: 24px;
        height: 24px;
        padding: 0;
        border: 1px solid var(--color-border);
        border-radius: 999px;
        background: color-mix(in srgb, var(--color-surface) 82%, transparent);
        backdrop-filter: var(--backdrop-blur);
        -webkit-backdrop-filter: var(--backdrop-blur);
        color: var(--color-text-secondary);
        box-shadow: 0 1px 4px rgba(0, 0, 0, 0.2);
        cursor: pointer;
        pointer-events: auto;
        touch-action: manipulation;
        transition:
          color var(--transition-fast, 0.15s),
          border-color var(--transition-fast, 0.15s),
          background var(--transition-fast, 0.15s),
          box-shadow var(--transition-fast, 0.15s);
      }
      .roll-btn svg {
        width: 14px;
        height: 14px;
        fill: currentColor;
      }
      .roll-btn:hover {
        color: var(--color-text-primary);
        border-color: var(--color-text-tertiary);
        background: var(--color-surface);
        box-shadow: 0 2px 6px rgba(0, 0, 0, 0.24);
      }
      .roll-btn:active {
        background: var(--color-surface-hover);
      }
      .roll-btn:focus-visible {
        outline: 2px solid var(--color-focus-ring, var(--color-primary));
        outline-offset: 1px;
      }
      /* Mirror the same rotate glyph so the two buttons read as opposite spins. */
      .roll-ccw {
        left: 4px;
      }
      .roll-cw {
        right: 4px;
      }
      .roll-cw svg {
        transform: scaleX(-1);
      }
    `,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ViewportCube {
  private readonly canvasRef = viewChild.required<ElementRef<HTMLCanvasElement>>('canvas');
  private readonly viewerControl = inject(ViewerControl);
  private readonly destroyRef = inject(DestroyRef);

  /** True while the camera looks straight at a face — reveals the rotate arrows. */
  protected readonly faceOn = signal(false);

  private renderer: WebGLRenderer | null = null;
  private scene: Scene | null = null;
  private camera: PerspectiveCamera | null = null;
  private cubeGroup: Group | null = null;
  private zones: Mesh[] = [];
  private axesGizmo: Group | null = null;
  private raycaster = new Raycaster();
  private rafHandle = 0;
  private hoveredKey: string | null = null;
  private resizeObserver: ResizeObserver | null = null;
  private themeObserver: MutationObserver | null = null;

  /**
   * Dirty flag for on-demand rendering. The cube is a static helper — most
   * frames it does not need to redraw at all. We render only when the
   * mirrored camera state changes, the hover state changes, the canvas
   * resizes, or the theme repaints the textures. This keeps an idle iPad
   * from burning a second WebGL pipeline at 60 fps for nothing.
   */
  private needsRender = true;
  private readonly lastRenderedDirection = new Vector3(NaN, NaN, NaN);
  private readonly lastRenderedUp = new Vector3(NaN, NaN, NaN);
  private lastRenderedFov = NaN;

  private dragging = false;
  private pointerId: number | null = null;
  private dragStart = new Vector2();
  private dragLast = new Vector2();
  private dragMoved = false;

  constructor() {
    afterNextRender(() => this.init());

    this.destroyRef.onDestroy(() => {
      cancelAnimationFrame(this.rafHandle);
      this.resizeObserver?.disconnect();
      this.themeObserver?.disconnect();
      if (this.cubeGroup) {
        disposeGroup(this.cubeGroup);
      }
      if (this.axesGizmo) {
        disposeGroup(this.axesGizmo);
      }
      this.renderer?.dispose();
    });
  }

  private init(): void {
    const canvas = this.canvasRef().nativeElement;

    this.renderer = new WebGLRenderer({
      canvas,
      alpha: true,
      // At Retina DPR (≥2) MSAA cost is non-trivial on iOS for a UI gizmo
      // this small — the labels are antialiased through the canvas-2D path
      // already.
      antialias: window.devicePixelRatio < 2,
      powerPreference: 'high-performance',
    });
    // Cap DPR for the same reason as the main viewer: iPads / phones at
    // DPR 3 quadruple the fragment cost for no perceptible benefit on a
    // 96×96 CSS-pixel widget.
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.setClearColor(0x000000, 0);

    this.scene = new Scene();
    this.scene.background = null;

    // Perspective camera that mirrors the main scene's FOV every frame so
    // the cube reads as orthographic when the main view is ortho (FOV ≈ 1°)
    // and as perspective when the main view is perspective (FOV ≈ 45°).
    // Initial parameters are placeholders — `resize()` and `tick()` set the
    // real aspect / FOV / distance.
    this.camera = new PerspectiveCamera(45, 1, 0.01, 100);
    this.camera.up.set(0, 0, 1);
    this.camera.position.set(0, 0, CUBE_DISTANCE);
    this.camera.lookAt(0, 0, 0);

    this.buildCube();

    this.axesGizmo = buildAxesGizmo();
    this.scene.add(this.axesGizmo);

    this.resize();
    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(canvas);

    canvas.addEventListener('pointerdown', this.onPointerDown);
    canvas.addEventListener('pointermove', this.onPointerMove);
    canvas.addEventListener('pointerup', this.onPointerUp);
    canvas.addEventListener('pointercancel', this.onPointerUp);
    canvas.addEventListener('pointerleave', this.onPointerLeave);

    // Re-paint face textures whenever the global theme changes so the cube
    // always picks up the current `--color-surface` / `--color-text-primary`
    // / `--color-border` tokens (matching the toolbar buttons).
    this.themeObserver = new MutationObserver(() => this.refreshTheme());
    this.themeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['class', 'style'],
    });

    this.tick();
  }

  /** (Re)build the chamfered cube — faces, edge bevels, corner bevels. */
  private buildCube(): void {
    if (!this.scene) {
      return;
    }
    if (this.cubeGroup) {
      this.scene.remove(this.cubeGroup);
      disposeGroup(this.cubeGroup);
    }
    const { group, zones } = buildViewCube(readPalette());
    this.cubeGroup = group;
    this.zones = zones;
    this.scene.add(group);
    if (this.hoveredKey) {
      const mesh = this.zoneByKey(this.hoveredKey);
      if (mesh) {
        applyZoneHover(mesh, true);
      }
    }
    this.needsRender = true;
  }

  /**
   * Repaint face textures + bevel tints when the theme changes, without
   * rebuilding geometry (theme mutations can fire often; geometry churn would
   * be wasteful).
   */
  private refreshTheme(): void {
    const palette = readPalette();
    const bevelBase = cssColor(palette.bevelBase);
    const bevelHover = cssColor(palette.bevelHover);
    for (const mesh of this.zones) {
      const ud = mesh.userData as ZoneUserData;
      const mat = mesh.material as MeshBasicMaterial;
      const hovered = ud.key === this.hoveredKey;
      if (ud.kind === 'face') {
        mat.map?.dispose();
        mat.map = makeFaceTexture(ud.label ?? '', hovered, palette);
        mat.needsUpdate = true;
      } else {
        ud.baseColor = bevelBase.clone();
        ud.hoverColor = bevelHover.clone();
        mat.color.copy(hovered ? ud.hoverColor : ud.baseColor);
        mat.opacity = hovered
          ? (ud.hoverOpacity ?? BEVEL_HOVER_OPACITY)
          : (ud.baseOpacity ?? BEVEL_OPACITY);
        mat.needsUpdate = true;
      }
    }
    this.needsRender = true;
  }

  private zoneByKey(key: string): Mesh | undefined {
    return this.zones.find((z) => (z.userData as ZoneUserData).key === key);
  }

  private resize(): void {
    if (!this.renderer || !this.camera) {
      return;
    }
    const canvas = this.canvasRef().nativeElement;
    const w = Math.max(canvas.clientWidth, 1);
    const h = Math.max(canvas.clientHeight, 1);
    this.renderer.setSize(w, h, false);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    this.needsRender = true;
  }

  private tick = (): void => {
    this.rafHandle = requestAnimationFrame(this.tick);
    if (!this.renderer || !this.scene || !this.camera || !this.cubeGroup) {
      return;
    }
    // Mirror the main viewer's camera orientation: place our fixed-distance
    // camera along the same direction (target → camera) the main camera is
    // viewing from, with a matching up vector. Detect changes vs. the last
    // rendered state so we can skip the (otherwise per-frame) render call
    // when the user is doing nothing — a major battery / thermal win on
    // iPads where this would otherwise run a second WebGL pipeline at the
    // display refresh rate.
    const state = this.viewerControl.cameraState;
    this.updateFaceOn(state.direction);
    if (
      !this.lastRenderedDirection.equals(state.direction) ||
      !this.lastRenderedUp.equals(state.up) ||
      this.lastRenderedFov !== state.fov
    ) {
      this.lastRenderedDirection.copy(state.direction);
      this.lastRenderedUp.copy(state.up);
      this.lastRenderedFov = state.fov;
      this.needsRender = true;
    }
    if (!this.needsRender) {
      return;
    }
    // Mirror the main camera's FOV. Distance from the cube is then derived
    // from FOV so the cube + axes gizmo always inscribe the same fraction
    // of the viewport regardless of projection — narrow FOV = far camera
    // (visually orthographic), wide FOV = closer camera (visually perspective).
    const fovRad = (state.fov * Math.PI) / 180;
    const fitRadius = CUBE_HALF_EXTENT * FRUSTUM_PADDING;
    // Account for non-square aspects: the limiting half-fov is on the
    // smaller axis (vertical FOV by default; horizontal scales by aspect).
    const aspect = this.camera.aspect;
    const vHalfFov = fovRad / 2;
    const hHalfFov = Math.atan(Math.tan(vHalfFov) * aspect);
    const limitHalfFov = Math.min(vHalfFov, hHalfFov);
    const distance = fitRadius / Math.tan(limitHalfFov);
    this.camera.fov = state.fov;
    this.camera.near = Math.max(distance - fitRadius * 2, 0.01);
    this.camera.far = distance + fitRadius * 2;
    this.camera.updateProjectionMatrix();
    this.camera.position.copy(state.direction).multiplyScalar(distance);
    this.camera.up.copy(state.up);
    this.camera.lookAt(0, 0, 0);
    this.renderer.render(this.scene, this.camera);
    this.needsRender = false;
  };

  private onPointerDown = (event: PointerEvent): void => {
    if (event.button !== 0) {
      return;
    }
    const canvas = this.canvasRef().nativeElement;
    canvas.setPointerCapture(event.pointerId);
    this.pointerId = event.pointerId;
    this.dragging = true;
    this.dragMoved = false;
    this.dragStart.set(event.clientX, event.clientY);
    this.dragLast.copy(this.dragStart);
    canvas.classList.add('is-dragging');
    canvas.style.cursor = 'grabbing';
  };

  private onPointerMove = (event: PointerEvent): void => {
    if (this.dragging && event.pointerId === this.pointerId) {
      const dx = event.clientX - this.dragLast.x;
      const dy = event.clientY - this.dragLast.y;
      this.dragLast.set(event.clientX, event.clientY);
      if (
        !this.dragMoved &&
        Math.hypot(event.clientX - this.dragStart.x, event.clientY - this.dragStart.y) >
          CLICK_DRAG_THRESHOLD
      ) {
        this.dragMoved = true;
        // Cancel any face hover styling once a drag begins.
        this.setHover(null);
      }
      if (this.dragMoved) {
        // dx > 0 = drag right → positive azimuth → camera orbits CCW from above → RIGHT face shown.
        // dy > 0 = drag down → positive polar → newPhi = phi − polar decreases → camera rises toward TOP.
        // (natural "grab-and-tilt": dragging up pulls the front face up, revealing the bottom)
        this.viewerControl.orbitSink?.(dx * DRAG_SENSITIVITY, dy * DRAG_SENSITIVITY);
      }
      return;
    }
    // Hover highlighting only when not dragging.
    this.setHover(this.pickZone(event));
  };

  private onPointerUp = (event: PointerEvent): void => {
    if (!this.dragging || event.pointerId !== this.pointerId) {
      return;
    }
    const canvas = this.canvasRef().nativeElement;
    if (canvas.hasPointerCapture(event.pointerId)) {
      canvas.releasePointerCapture(event.pointerId);
    }
    canvas.classList.remove('is-dragging');
    const wasDrag = this.dragMoved;
    this.dragging = false;
    this.pointerId = null;
    this.dragMoved = false;

    if (!wasDrag) {
      const mesh = this.pickZone(event);
      if (mesh) {
        const ud = mesh.userData as ZoneUserData;
        // Snapping to a cube face/edge/corner also flattens the projection to
        // orthographic (CAD convention); a later free pan/zoom reverts it.
        this.viewerControl.lookFrom(ud.direction, WORLD_UP, true);
      }
    }
    canvas.style.cursor = this.pickZone(event) ? 'pointer' : 'grab';
  };

  private onPointerLeave = (): void => {
    if (!this.dragging) {
      this.setHover(null);
    }
  };

  private setHover(mesh: Mesh | null): void {
    const key = mesh ? (mesh.userData as ZoneUserData).key : null;
    if (key === this.hoveredKey) {
      return;
    }
    if (this.hoveredKey) {
      const prev = this.zoneByKey(this.hoveredKey);
      if (prev) {
        applyZoneHover(prev, false);
      }
    }
    if (mesh) {
      applyZoneHover(mesh, true);
    }
    this.hoveredKey = key;
    if (!this.dragging) {
      this.canvasRef().nativeElement.style.cursor = mesh ? 'pointer' : 'grab';
    }
    this.needsRender = true;
  }

  private pickZone(event: PointerEvent): Mesh | null {
    if (!this.camera || this.zones.length === 0) {
      return null;
    }
    const canvas = this.canvasRef().nativeElement;
    const rect = canvas.getBoundingClientRect();
    const ndc = new Vector2(
      ((event.clientX - rect.left) / rect.width) * 2 - 1,
      -((event.clientY - rect.top) / rect.height) * 2 + 1,
    );
    this.raycaster.setFromCamera(ndc, this.camera);
    const hits = this.raycaster.intersectObjects(this.zones, false);
    return hits.length > 0 ? (hits[0].object as Mesh) : null;
  }

  /** Toggle the rotate arrows when the camera looks (nearly) straight at a face. */
  private updateFaceOn(direction: Vector3): void {
    let best = -Infinity;
    for (const c of CARDINALS) {
      const d = direction.dot(c);
      if (d > best) {
        best = d;
      }
    }
    const on = best >= FACE_ON_COS;
    if (on !== this.faceOn()) {
      this.faceOn.set(on);
    }
  }

  /**
   * Roll the view 90° about its own axis. `cw` rolls the model clockwise on
   * screen, `ccw` counter-clockwise. Orbiting afterwards stays consistent
   * because the main orbit works in the camera-up frame.
   */
  protected roll(direction: 'cw' | 'ccw'): void {
    const quarter = Math.PI / 2;
    this.viewerControl.roll(direction === 'cw' ? -quarter : quarter);
  }
}

/**
 * Snapshot of the themed colour tokens used to paint a cube face. Captured
 * once per repaint so all six faces stay visually consistent even mid-theme-
 * transition.
 */
interface CubePalette {
  faceBase: string;
  faceHover: string;
  text: string;
  bevelBase: string;
  bevelHover: string;
}

/**
 * Read the current theme tokens from `<html>` computed styles. Uses only
 * neutral graphite surface / border / text tokens (never the amber accent) so
 * the cube reads as a quiet OS-native widget in both dark and light mode; the
 * hover state is a subtle neutral lift rather than an accent tint.
 */
function readPalette(): CubePalette {
  const styles = getComputedStyle(document.documentElement);
  const get = (name: string, fallback: string): string =>
    styles.getPropertyValue(name).trim() || fallback;
  return {
    faceBase: get('--color-surface', '#1b1c20'),
    faceHover: get('--color-text-tertiary', '#7c808a'),
    text: get('--color-text-secondary', '#b7bac1'),
    bevelBase: get('--color-border', '#2b2e34'),
    bevelHover: get('--color-text-tertiary', '#7c808a'),
  };
}

/**
 * Build a CanvasTexture for a single cube face: a flat, borderless graphite
 * fill that lifts to a slightly brighter neutral on hover, with a themed
 * (non-accent) label. No accent colour is used anywhere.
 */
function makeFaceTexture(label: string, hovered: boolean, palette: CubePalette): CanvasTexture {
  const size = 256;
  const canvas = document.createElement('canvas');
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext('2d');
  if (!ctx) {
    return new CanvasTexture(canvas);
  }

  // Flat, borderless fill covering the whole face. On hover the face lifts to
  // a clear mid graphite (matching the edge/corner hover) and the label flips
  // to the base surface colour so it stays legible against the lighter fill in
  // both dark and light themes — an inverted "highlighted button" look.
  ctx.fillStyle = hovered ? palette.faceHover : palette.faceBase;
  ctx.fillRect(0, 0, size, size);

  ctx.fillStyle = hovered ? palette.faceBase : palette.text;
  // Monospace so every face label has identical letter geometry, keeping the
  // cube reading like a uniform button grid.
  ctx.font = '700 64px "IBM Plex Mono", "SF Mono", Menlo, Consolas, ui-monospace, monospace';
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  // Face-tile orientation (which world direction is "up") is baked into the
  // plane's basis in `buildFaceTile`, so the label is always drawn upright.
  ctx.fillText(label, size / 2, size / 2);

  const tex = new CanvasTexture(canvas);
  tex.minFilter = LinearFilter;
  tex.magFilter = LinearFilter;
  tex.needsUpdate = true;
  return tex;
}

/**
 * Build the small RGB axes gizmo that sits at the cube's -X/-Y/-Z corner
 * ("bottom-left-back"). The three coloured shafts run **along** the three
 * cube edges meeting at that corner, doubling as dimensional guides for the
 * build-volume orientation: red = X, green = Y, blue = Z, each ending in a
 * matching arrow head + billboarded sprite label so the user can always
 * read which colour maps to which axis regardless of camera angle.
 *
 * The gizmo lives in the same scene as the cube — depth testing against
 * the cube's opaque faces naturally hides the parts that should be behind
 * the cube when viewed from the opposite octant.
 */
function buildAxesGizmo(): Group {
  const group = new Group();
  const half = CUBE_SIZE / 2;
  const eps = AXIS_EDGE_OFFSET;

  // Each axis runs along the cube edge starting at -half on its own axis,
  // offset by `eps` outboard on the two perpendicular axes so the shaft sits
  // just outside the cube faces (no z-fighting with the textured face).
  const xOrigin = new Vector3(-half, -half - eps, -half - eps);
  const yOrigin = new Vector3(-half - eps, -half, -half - eps);
  const zOrigin = new Vector3(-half - eps, -half - eps, -half);

  group.add(buildAxisArrow('X', new Vector3(1, 0, 0), AXIS_COLOR_X, xOrigin));
  group.add(buildAxisArrow('Y', new Vector3(0, 1, 0), AXIS_COLOR_Y, yOrigin));
  group.add(buildAxisArrow('Z', new Vector3(0, 0, 1), AXIS_COLOR_Z, zOrigin));

  return group;
}

/**
 * Build one axis arrow (start tick + shaft + head + label sprite) pointing
 * along `direction` from `origin`. `direction` must be a unit cardinal vector.
 * The shaft length spans the full cube edge so the arrow visually annotates
 * the edge as a dimension line.
 */
function buildAxisArrow(label: string, direction: Vector3, color: number, origin: Vector3): Group {
  const arrow = new Group();
  const shaftLength = AXIS_LENGTH - AXIS_HEAD_LENGTH;

  // Depth-tested opaque material. The cube's faces are now opaque, so the
  // shafts that run along back edges are occluded by the body while the
  // outboard front-edge shafts stay visible past the silhouette.
  const material = new MeshBasicMaterial({ color });

  // Perpendicular tick at the origin end — architectural dimension-line cap
  // marking the start of the measured span. Built along +X (perpendicular to
  // the shaft's local +Y) so it sits flat against the cube corner.
  const tickGeometry = new CylinderGeometry(
    AXIS_TICK_RADIUS,
    AXIS_TICK_RADIUS,
    AXIS_TICK_LENGTH,
    8,
  );
  const tick = new Mesh(tickGeometry, material);
  tick.rotation.z = Math.PI / 2;
  arrow.add(tick);

  // Shaft — CylinderGeometry's default axis is +Y, so build along +Y and
  // rotate the whole arrow into place via setFromUnitVectors below.
  const shaftGeometry = new CylinderGeometry(AXIS_SHAFT_RADIUS, AXIS_SHAFT_RADIUS, shaftLength, 16);
  const shaft = new Mesh(shaftGeometry, material);
  shaft.position.y = shaftLength / 2;
  arrow.add(shaft);

  // Arrow head sits on top of the shaft.
  const headGeometry = new ConeGeometry(AXIS_HEAD_RADIUS, AXIS_HEAD_LENGTH, 16);
  const head = new Mesh(headGeometry, material);
  head.position.y = shaftLength + AXIS_HEAD_LENGTH / 2;
  arrow.add(head);

  // Billboarded label sprite — always faces the camera, sits just past the
  // arrow tip in the arrow's local +Y direction (rotated into world space
  // alongside the rest of the arrow). Standard depth testing means the opaque
  // cube body occludes labels on its far side.
  const sprite = new Sprite(
    new SpriteMaterial({
      map: makeAxisLabelTexture(label, color),
      transparent: true,
    }),
  );
  sprite.position.y = AXIS_LENGTH + AXIS_LABEL_OFFSET;
  sprite.scale.set(AXIS_LABEL_SIZE, AXIS_LABEL_SIZE, 1);
  arrow.add(sprite);

  // Orient the +Y-aligned arrow so its tip points along `direction`.
  arrow.position.copy(origin);
  arrow.quaternion.setFromUnitVectors(new Vector3(0, 1, 0), direction);

  return arrow;
}

/**
 * Build a CanvasTexture for an axis label sprite. The label is drawn in the
 * matching axis colour on a transparent background.
 */
function makeAxisLabelTexture(label: string, color: number): CanvasTexture {
  const size = 128;
  const canvas = document.createElement('canvas');
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext('2d');
  if (!ctx) {
    return new CanvasTexture(canvas);
  }

  const hex = `#${color.toString(16).padStart(6, '0')}`;
  ctx.font = '700 96px "Plus Jakarta Sans", "Avenir Next", "Segoe UI", system-ui, sans-serif';
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  // Subtle dark halo for legibility against light cube faces.
  ctx.lineWidth = 8;
  ctx.strokeStyle = 'rgba(0, 0, 0, 0.55)';
  ctx.strokeText(label, size / 2, size / 2);
  ctx.fillStyle = hex;
  ctx.fillText(label, size / 2, size / 2);

  const tex = new CanvasTexture(canvas);
  tex.minFilter = LinearFilter;
  tex.magFilter = LinearFilter;
  tex.needsUpdate = true;
  return tex;
}

/** Recursively dispose every Mesh / Sprite resource (geometry, material, map). */
function disposeGroup(root: Group): void {
  root.traverse((obj) => {
    if (obj instanceof Mesh) {
      obj.geometry.dispose();
      const mats = Array.isArray(obj.material) ? obj.material : [obj.material];
      for (const m of mats) {
        const mm = m as MeshBasicMaterial;
        mm.map?.dispose();
        mm.dispose();
      }
    } else if (obj instanceof Sprite) {
      const mat = obj.material;
      mat.map?.dispose();
      mat.dispose();
    }
  });
}

// ---------------------------------------------------------------------------
// ViewCube geometry — a chamfered cube of 26 clickable zones
// ---------------------------------------------------------------------------

/**
 * Build the full chamfered ViewCube: 6 labelled face tiles, 12 edge bevels and
 * 8 corner bevels. Each returned mesh carries {@link ZoneUserData} describing
 * the view it snaps to and how it highlights on hover.
 */
function buildViewCube(palette: CubePalette): { group: Group; zones: Mesh[] } {
  const group = new Group();
  const zones: Mesh[] = [];
  const bevelBase = cssColor(palette.bevelBase);
  const bevelHover = cssColor(palette.bevelHover);

  // --- Faces (6) ---
  for (const def of FACE_DEFS) {
    const mesh = buildFaceTile(def, makeFaceTexture(def.label, false, palette));
    mesh.userData = {
      kind: 'face',
      key: `face:${def.label}`,
      direction: def.normal.clone().normalize(),
      label: def.label,
    } satisfies ZoneUserData;
    zones.push(mesh);
    group.add(mesh);
  }

  // --- Edge bevels (12) ---
  for (const [a, b] of edgePairs()) {
    const c = new Vector3().crossVectors(a, b).normalize();
    const q1 = a
      .clone()
      .multiplyScalar(HALF)
      .addScaledVector(b, FACE_HALF)
      .addScaledVector(c, -FACE_HALF);
    const q2 = a
      .clone()
      .multiplyScalar(HALF)
      .addScaledVector(b, FACE_HALF)
      .addScaledVector(c, FACE_HALF);
    const q3 = a
      .clone()
      .multiplyScalar(FACE_HALF)
      .addScaledVector(b, HALF)
      .addScaledVector(c, FACE_HALF);
    const q4 = a
      .clone()
      .multiplyScalar(FACE_HALF)
      .addScaledVector(b, HALF)
      .addScaledVector(c, -FACE_HALF);
    const mesh = bevelMesh(quadGeometry(q1, q2, q3, q4), bevelBase);
    mesh.userData = {
      kind: 'edge',
      key: edgeKey(a, b),
      direction: a.clone().add(b).normalize(),
      baseColor: bevelBase.clone(),
      hoverColor: bevelHover.clone(),
      baseOpacity: BEVEL_OPACITY,
      hoverOpacity: BEVEL_HOVER_OPACITY,
    } satisfies ZoneUserData;
    zones.push(mesh);
    group.add(mesh);
  }

  // --- Corner bevels (8) ---
  for (const s of cornerSigns()) {
    const v1 = new Vector3(s.x * HALF, s.y * FACE_HALF, s.z * FACE_HALF);
    const v2 = new Vector3(s.x * FACE_HALF, s.y * HALF, s.z * FACE_HALF);
    const v3 = new Vector3(s.x * FACE_HALF, s.y * FACE_HALF, s.z * HALF);
    const mesh = bevelMesh(triGeometry(v1, v2, v3), bevelBase);
    mesh.userData = {
      kind: 'corner',
      key: `corner:${s.x}${s.y}${s.z}`,
      direction: new Vector3(s.x, s.y, s.z).normalize(),
      baseColor: bevelBase.clone(),
      hoverColor: bevelHover.clone(),
      baseOpacity: BEVEL_OPACITY,
      hoverOpacity: BEVEL_HOVER_OPACITY,
    } satisfies ZoneUserData;
    zones.push(mesh);
    group.add(mesh);
  }

  return { group, zones };
}

/** Apply / remove the hover highlight for a single zone mesh. */
function applyZoneHover(mesh: Mesh, hovered: boolean): void {
  const ud = mesh.userData as ZoneUserData;
  const mat = mesh.material as MeshBasicMaterial;
  if (ud.kind === 'face') {
    mat.map?.dispose();
    mat.map = makeFaceTexture(ud.label ?? '', hovered, readPalette());
    mat.needsUpdate = true;
  } else {
    mat.color.copy(hovered ? (ud.hoverColor ?? mat.color) : (ud.baseColor ?? mat.color));
    mat.opacity = hovered
      ? (ud.hoverOpacity ?? BEVEL_HOVER_OPACITY)
      : (ud.baseOpacity ?? BEVEL_OPACITY);
    mat.needsUpdate = true;
  }
}

/**
 * Build one labelled face tile: an inset square in the cube face plane, rotated
 * so its texture "up" (local +Y) aligns with `def.faceUp` and its front (local
 * +Z) points outward along `def.normal` — never mirrored.
 */
function buildFaceTile(def: FaceDef, texture: CanvasTexture): Mesh {
  const geo = new PlaneGeometry(2 * FACE_HALF, 2 * FACE_HALF);
  // Fully opaque with depth write — the cube occludes its own back faces so
  // there's nothing to "see behind" on hover.
  const mat = new MeshBasicMaterial({
    map: texture,
  });
  const mesh = new Mesh(geo, mat);
  const zAxis = def.normal.clone().normalize();
  const yAxis = def.faceUp.clone().normalize();
  const xAxis = new Vector3().crossVectors(yAxis, zAxis).normalize();
  const yOrtho = new Vector3().crossVectors(zAxis, xAxis).normalize();
  mesh.quaternion.setFromRotationMatrix(new Matrix4().makeBasis(xAxis, yOrtho, zAxis));
  mesh.position.copy(def.normal).multiplyScalar(HALF);
  return mesh;
}

/** Two-triangle quad from four coplanar corners (double-sided; winding-agnostic). */
function quadGeometry(a: Vector3, b: Vector3, c: Vector3, d: Vector3): BufferGeometry {
  const geo = new BufferGeometry();
  // prettier-ignore
  const p = new Float32Array([
    a.x, a.y, a.z, b.x, b.y, b.z, c.x, c.y, c.z,
    a.x, a.y, a.z, c.x, c.y, c.z, d.x, d.y, d.z,
  ]);
  geo.setAttribute('position', new Float32BufferAttribute(p, 3));
  return geo;
}

/** Single triangle from three corners. */
function triGeometry(a: Vector3, b: Vector3, c: Vector3): BufferGeometry {
  const geo = new BufferGeometry();
  // prettier-ignore
  const p = new Float32Array([a.x, a.y, a.z, b.x, b.y, b.z, c.x, c.y, c.z]);
  geo.setAttribute('position', new Float32BufferAttribute(p, 3));
  return geo;
}

/** An opaque, double-sided bevel facet used for edges and corners. */
function bevelMesh(geometry: BufferGeometry, color: Color): Mesh {
  return new Mesh(
    geometry,
    new MeshBasicMaterial({
      color: color.clone(),
      side: DoubleSide,
    }),
  );
}

/** The 12 unordered pairs of perpendicular cardinal axes (one per cube edge). */
function edgePairs(): [Vector3, Vector3][] {
  const pairs: [Vector3, Vector3][] = [];
  for (let i = 0; i < CARDINALS.length; i++) {
    for (let j = i + 1; j < CARDINALS.length; j++) {
      if (Math.abs(CARDINALS[i].dot(CARDINALS[j])) < 1e-6) {
        pairs.push([CARDINALS[i].clone(), CARDINALS[j].clone()]);
      }
    }
  }
  return pairs;
}

/** The 8 corner sign triples (±1, ±1, ±1). */
function cornerSigns(): { x: number; y: number; z: number }[] {
  const signs: { x: number; y: number; z: number }[] = [];
  for (const x of [-1, 1]) {
    for (const y of [-1, 1]) {
      for (const z of [-1, 1]) {
        signs.push({ x, y, z });
      }
    }
  }
  return signs;
}

/** Order-independent identity for the edge shared by two cardinal axes. */
function edgeKey(a: Vector3, b: Vector3): string {
  const k = (v: Vector3): string => `${v.x},${v.y},${v.z}`;
  return `edge:${[k(a), k(b)].sort().join('|')}`;
}

/** Resolve any CSS colour string (incl. 8-digit hex / rgba) to a Three.Color. */
function cssColor(css: string): Color {
  const canvas = document.createElement('canvas');
  canvas.width = 1;
  canvas.height = 1;
  const ctx = canvas.getContext('2d');
  if (!ctx) {
    return new Color(0x808080);
  }
  ctx.fillStyle = css;
  ctx.fillRect(0, 0, 1, 1);
  const [r, g, b] = ctx.getImageData(0, 0, 1, 1).data;
  return new Color(r / 255, g / 255, b / 255);
}
