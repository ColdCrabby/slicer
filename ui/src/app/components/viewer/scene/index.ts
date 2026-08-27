import {
  BoxGeometry,
  Box3,
  Color,
  DirectionalLight,
  Group,
  HemisphereLight,
  Mesh,
  MeshBasicMaterial,
  type Object3D,
  PerspectiveCamera,
  Scene,
  SRGBColorSpace,
  Vector3,
  WebGLRenderer,
  WebGLRenderTarget,
} from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import type { PrintAreaConfig } from '../../../services/print-area';
import type { ObjectMode, TwoFingerGesture } from '../../../services/viewer-control';
import { GizmoManager } from '../gizmo';
import { INITIAL_CAMERA_UP, INITIAL_PERSPECTIVE_FOV, SceneCamera } from './camera';
import { SceneControls } from './controls';
import { SceneGrid } from './grid';
import { PointerArbiter } from './pointer-arbiter';
import { SceneSelection } from './selection';
import type { SceneGizmoHandlers, SceneSelectionHandlers, ViewerView } from './types';
import { disposeObject } from './utils';

const CAMERA_NEAR = 0.1;
const CAMERA_FAR = 1_000_000;
const MAX_PIXEL_RATIO = 2;

function shouldDisableAntialias(): boolean {
  return typeof window !== 'undefined' && window.devicePixelRatio >= 2;
}

/**
 * Optional render-quality knobs sourced from user settings. Omitted fields
 * fall back to the historical defaults (auto anti-alias, 2× pixel-ratio cap).
 */
export interface ViewerSceneOptions {
  /** Force MSAA on/off. When undefined, disabled on high-DPI (≥2×) displays. */
  antialias?: boolean;
  /** Maximum device-pixel-ratio the renderer draws at. */
  pixelRatioCap?: number;
  /** Initial perspective field-of-view in degrees. */
  fieldOfView?: number;
}

/** Field-of-view (deg) used for the fixed-angle thumbnail render. */
const THUMBNAIL_FOV = 40;
/** Fit padding — a whisker of margin so the tight box-fit never clips an edge. */
const THUMBNAIL_FIT_PADDING = 1.02;

/**
 * Inputs for {@link ViewerScene.captureThumbnail}. The caller resolves the
 * world-space camera direction/up (from a fixed preset), the thumbnail theme,
 * and the background — the scene handles framing, an off-screen render, and
 * full state restoration.
 */
export interface ThumbnailCaptureOptions {
  /** Square output edge length in pixels. */
  sizePx: number;
  /** Normalised world-space camera direction (target → camera). */
  direction: Vector3;
  /** World-space camera up vector. */
  up: Vector3;
  /** Whether the thumbnail uses the dark studio lighting rig. */
  isDark: boolean;
  /** The live scene theme to restore after the capture. */
  liveIsDark: boolean;
  /** Solid background colour (hex), or `null` for a transparent background. */
  background: number | null;
  /**
   * Explicit objects to frame + render in isolation — typically freshly-built
   * model meshes. When provided, all live scene content (model *and* G-code
   * preview) is hidden for the capture and only these subjects are drawn, so
   * the thumbnail always depicts the model regardless of the viewer's current
   * mode. When omitted, the live {@link ViewerScene.contentRoot} is framed.
   */
  subjects?: Object3D[];
}

/**
 * Owns the Three.js scene, camera, renderer, and render loop. All
 * mode-specific responsibilities are delegated to focused sub-modules:
 * - {@link SceneCamera} — camera animations and view presets
 * - {@link SceneControls} — orbit controls, touch, and autoscroll
 * - {@link SceneGrid} — adaptive build-plate grid
 * - {@link SceneSelection} — selectable objects, highlight, raycasting
 *
 * Mode-specific content (mesh / G-code lines) is added to {@link contentRoot}.
 * The scene itself is created once and reused across mode switches so the
 * WebGL context, camera state, and controls are not re-initialised.
 */
export class ViewerScene {
  readonly scene = new Scene();
  readonly camera: PerspectiveCamera;
  readonly renderer: WebGLRenderer;
  readonly controls: OrbitControls;
  readonly contentRoot = new Group();

  private readonly host: HTMLElement;
  private readonly resizeObserver: ResizeObserver;
  private readonly _camera: SceneCamera;
  private readonly _controls: SceneControls;
  private readonly _grid: SceneGrid;
  private readonly _selection: SceneSelection;
  private readonly _pointerArbiter: PointerArbiter;
  private readonly gizmo: GizmoManager;
  private readonly axesGizmo: Group;
  private readonly hemiLight: HemisphereLight;
  private readonly keyLight: DirectionalLight;
  private readonly fillLight: DirectionalLight;

  private rafHandle = 0;
  private disposed = false;
  private lastFrameTime = 0;
  private smoothedFps = 0;
  private smoothedDelayMs = 0;
  private lastFpsPublishTime = 0;
  private pixelRatioCap = MAX_PIXEL_RATIO;

  /**
   * Sink called at the end of every rendered frame with the live camera
   * direction (target→camera normalised), up vector, and FOV. Used by
   * the viewport-cube gizmo.
   */
  cameraStateSink: ((direction: Vector3, up: Vector3, fov: number) => void) | null = null;

  /**
   * Sink called approximately once per second with the smoothed FPS and
   * average frame delay in ms.
   */
  fpsSink: ((fps: number, delayMs: number) => void) | null = null;

  set selectionHandlers(h: SceneSelectionHandlers | null) {
    this._selection.selectionHandlers = h;
  }

  set gizmoHandlers(h: SceneGizmoHandlers | null) {
    this._selection.gizmoHandlers = h;
  }

  constructor(host: HTMLElement, initialPrintArea?: PrintAreaConfig, options?: ViewerSceneOptions) {
    this.host = host;
    this.pixelRatioCap = options?.pixelRatioCap ?? MAX_PIXEL_RATIO;
    const printArea: PrintAreaConfig = initialPrintArea ?? {
      bedShape: 'rectangular',
      printableAreaWidth: 220,
      printableAreaHeight: 220,
      movableAreaX: 0,
      movableAreaY: 0,
    };

    this.scene.background = null;
    this.scene.add(this.contentRoot);

    const { clientWidth, clientHeight } = this.sizeOf(host);
    this.camera = new PerspectiveCamera(
      options?.fieldOfView ?? INITIAL_PERSPECTIVE_FOV,
      clientWidth / clientHeight,
      CAMERA_NEAR,
      CAMERA_FAR,
    );
    this.camera.up.copy(INITIAL_CAMERA_UP);
    const initialPose = SceneCamera.computeInitialPose(printArea);
    this.camera.position.copy(initialPose.position);
    this.camera.lookAt(initialPose.target);

    this.renderer = new WebGLRenderer({
      antialias: options?.antialias ?? !shouldDisableAntialias(),
      alpha: true,
      powerPreference: 'high-performance',
    });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, this.pixelRatioCap));
    this.renderer.setClearColor(0x000000, 0);
    this.renderer.setSize(clientWidth, clientHeight);
    this.renderer.domElement.style.touchAction = 'none';
    host.appendChild(this.renderer.domElement);

    // Palm rejection is installed on `host` (an ancestor of the canvas) in the
    // capture phase so it runs before OrbitControls, selection, and the
    // two-finger touch handlers — it can veto a palm/wrist contact before any
    // of them start a camera gesture. Created before those consumers so its
    // capture listeners are the first thing every pointer event meets.
    this._pointerArbiter = new PointerArbiter(host);

    this.controls = new OrbitControls(this.camera, this.renderer.domElement);
    this.controls.enableDamping = false;
    this.controls.zoomToCursor = false;
    this.controls.screenSpacePanning = true;
    this.controls.target.copy(initialPose.target);
    this.controls.zoomSpeed = 2.5;
    this.controls.minDistance = 1;
    this.controls.maxDistance = 100_000;

    // Construction order:
    //   SceneCamera → GizmoManager → SceneSelection (needs gizmo)
    //   → SceneControls (needs cancelDrag callback) → SceneGrid
    this._camera = new SceneCamera(this.camera, this.controls, this.contentRoot, printArea);
    this.gizmo = new GizmoManager(this.scene, this.camera, this.renderer);
    this._selection = new SceneSelection(this.scene, this.camera, this.renderer, this.gizmo);

    this._controls = new SceneControls(this.camera, this.controls, this.renderer, () =>
      this._selection.cancelActiveDrag(),
    );
    // Free pan/zoom in the main viewport reverts the viewport-cube's temporary
    // orthographic snap; rotate and cube gestures never fire this.
    this._controls.setRevertGestureSink(() => this._camera.notifyUserPanOrZoom());
    this._grid = new SceneGrid(this.scene, this.camera, this.controls, this.renderer, printArea);

    // Wire gizmo callbacks.
    this.gizmo.onDragStart = () => {
      this.controls.enabled = false;
    };
    this.gizmo.onDelta = (delta) => {
      const ids = Array.from(this._selection.selectedIds) as string[];
      this._selection.gizmoHandlers?.delta(ids, delta);
    };
    this.gizmo.onDragEnd = () => {
      this.controls.enabled = true;
      this._selection.gizmoHandlers?.end();
    };

    // Lights — a soft studio rig. A hemisphere fill lifts the shadowed faces
    // off the background so the model never reads as a murky silhouette, a key
    // light sculpts the form, and a dim opposite fill keeps back faces legible.
    // Colours/intensities are theme-tuned in setTheme() below.
    this.hemiLight = new HemisphereLight(0xffffff, 0x9a9ea8, 0.6);
    this.scene.add(this.hemiLight);
    this.keyLight = new DirectionalLight(0xffffff, 0.8);
    this.keyLight.position.set(200, 300, 400);
    this.scene.add(this.keyLight);
    this.fillLight = new DirectionalLight(0xffffff, 0.25);
    this.fillLight.position.set(-180, 140, -220);
    this.scene.add(this.fillLight);
    this.setTheme(true);

    this.axesGizmo = buildAxesGizmo(40, 0.6);
    this.scene.add(this.axesGizmo);

    this.resizeObserver = new ResizeObserver(() => this.handleResize());
    this.resizeObserver.observe(host);

    this.tick();
  }

  // -------------------------------------------------------------------------
  // Public API — delegates to sub-modules
  // -------------------------------------------------------------------------

  setPrintArea(config: PrintAreaConfig): void {
    this._camera.setPrintArea(config);
    this._grid.setPrintArea(config);
  }

  /**
   * Retune the lighting rig for the active colour scheme. Light mode needs a
   * brighter hemisphere fill and a mid-grey ground tint so the model keeps
   * readable form against the near-white background instead of collapsing into
   * a dark shape; dark mode drops the fill so the faces don't wash out against
   * the near-black background.
   */
  setTheme(isDark: boolean): void {
    if (isDark) {
      this.hemiLight.groundColor.setHex(0x4a4e57);
      this.hemiLight.intensity = 0.72;
      this.keyLight.intensity = 0.9;
      this.fillLight.color.setHex(0xd6deec);
      this.fillLight.intensity = 0.26;
    } else {
      this.hemiLight.groundColor.setHex(0xb2b8c2);
      this.hemiLight.intensity = 1.3;
      this.keyLight.intensity = 1.25;
      this.fillLight.color.setHex(0xffffff);
      this.fillLight.intensity = 0.34;
    }
  }

  clearContent(): void {
    this._selection.cancelActiveDrag();
    this._selection.clearAll();
    for (let i = this.contentRoot.children.length - 1; i >= 0; i--) {
      const child = this.contentRoot.children[i];
      this.contentRoot.remove(child);
      disposeObject(child);
    }
  }

  registerSelectable(id: string, object: Object3D): void {
    this._selection.register(id, object);
  }

  unregisterSelectable(id: string): void {
    this._selection.unregister(id);
  }

  clearSelectables(): void {
    this._selection.clearAll();
  }

  setSelectedIds(ids: ReadonlySet<string>): void {
    this._selection.setSelectedIds(ids);
  }

  setObjectTransform(
    id: string,
    transform: {
      position: { x: number; y: number; z: number };
      rotation: { x: number; y: number; z: number };
      scale: { x: number; y: number; z: number };
    },
  ): void {
    this._selection.setObjectTransform(id, transform);
  }

  fitToContent(padding?: number): void {
    this._camera.fitToContent(padding);
  }

  setView(view: ViewerView): void {
    this._camera.setView(view);
  }

  resetView(): void {
    this._camera.resetView();
  }

  /**
   * Render the current model content to a square PNG from a fixed camera angle
   * and theme, entirely off-screen. The live canvas, camera, controls, lights,
   * and background are all restored before returning, so this never disturbs
   * the on-screen view. Returns a `data:image/png;base64,…` URL, or `null` when
   * there is nothing to frame.
   */
  captureThumbnail(options: ThumbnailCaptureOptions): string | null {
    const size = Math.max(16, Math.round(options.sizePx));

    // When explicit subjects are given (freshly-built model meshes), render
    // them in isolation from a temporary group so neither the live model nor
    // the G-code preview toolpaths bleed into the shot or skew the framing.
    const subjects = options.subjects ?? [];
    let subjectGroup: Group | null = null;
    if (subjects.length > 0) {
      subjectGroup = new Group();
      for (const s of subjects) {
        subjectGroup.add(s);
      }
      this.scene.add(subjectGroup);
    }
    const frameTarget: Object3D = subjectGroup ?? this.contentRoot;
    frameTarget.updateMatrixWorld(true);

    const box = new Box3().setFromObject(frameTarget);
    if (box.isEmpty()) {
      if (subjectGroup) {
        this.scene.remove(subjectGroup);
      }
      return null;
    }
    const center = new Vector3();
    box.getCenter(center);

    // Build the camera basis (forward = viewing direction) so we can fit the
    // eight box corners tightly — the loose bounding sphere would leave a big
    // border, especially for elongated prints.
    const dir = options.direction.clone().normalize();
    const forward = dir.clone().negate();
    const right = new Vector3().crossVectors(forward, options.up);
    if (right.lengthSq() < 1e-8) {
      right.set(1, 0, 0);
    }
    right.normalize();
    const trueUp = new Vector3().crossVectors(right, forward).normalize();

    const fovRad = (THUMBNAIL_FOV * Math.PI) / 180;
    const tanHalf = Math.tan(fovRad / 2);
    const corner = new Vector3();
    let distance = 0;
    let maxExtent = 1;
    for (let cx = 0; cx < 2; cx++) {
      for (let cy = 0; cy < 2; cy++) {
        for (let cz = 0; cz < 2; cz++) {
          corner.set(
            cx ? box.max.x : box.min.x,
            cy ? box.max.y : box.min.y,
            cz ? box.max.z : box.min.z,
          );
          corner.sub(center);
          const px = corner.dot(right);
          const py = corner.dot(trueUp);
          const pf = corner.dot(forward);
          // Distance at which this corner just fits the square frustum.
          const needed = Math.max(Math.abs(px), Math.abs(py)) / tanHalf - pf;
          distance = Math.max(distance, needed);
          maxExtent = Math.max(maxExtent, Math.abs(px), Math.abs(py), Math.abs(pf));
        }
      }
    }
    distance = Math.max(distance * THUMBNAIL_FIT_PADDING, 0.01);

    const cam = new PerspectiveCamera(THUMBNAIL_FOV, 1, CAMERA_NEAR, CAMERA_FAR);
    cam.up.copy(options.up);
    cam.position.copy(center).addScaledVector(dir, distance);
    cam.near = Math.max(distance - maxExtent * 2, 0.01);
    cam.far = distance + maxExtent * 4;
    cam.lookAt(center);
    cam.updateProjectionMatrix();

    // Hide everything that isn't the frame target or a light (grid, axes,
    // gizmos — and, when rendering isolated subjects, the live contentRoot too).
    const keep = new Set<Object3D>([frameTarget, this.hemiLight, this.keyLight, this.fillLight]);
    const rehide: Object3D[] = [];
    for (const child of this.scene.children) {
      if (!keep.has(child) && child.visible) {
        child.visible = false;
        rehide.push(child);
      }
    }

    // Apply the thumbnail theme, then a solid studio background or a fully
    // transparent one (`background === null` → the renderer's 0-alpha clear).
    const prevBackground = this.scene.background;
    this.setTheme(options.isDark);
    this.scene.background = options.background === null ? null : new Color(options.background);
    // Drop the emissive selection glow so a selected object isn't captured lit up.
    this._selection.setHighlightVisible(false);

    // Disable frustum culling on the framed content for the duration of the
    // render. G-code preview is drawn with InstancedMesh whose cull volume can
    // be stale/degenerate, so from the thumbnail camera geometry could be
    // wrongly culled and the image comes out blank. Culling is a live-view perf
    // optimisation we don't need here.
    const unculled: Object3D[] = [];
    frameTarget.traverse((node) => {
      if (node.frustumCulled) {
        node.frustumCulled = false;
        unculled.push(node);
      }
    });

    const target = new WebGLRenderTarget(size, size, { samples: 4 });
    target.texture.colorSpace = SRGBColorSpace;
    const prevTarget = this.renderer.getRenderTarget();
    const buffer = new Uint8Array(size * size * 4);
    let dataUrl: string | null = null;
    try {
      this.renderer.setRenderTarget(target);
      this.renderer.clear();
      this.renderer.render(this.scene, cam);
      this.renderer.readRenderTargetPixels(target, 0, 0, size, size, buffer);
      dataUrl = encodePixelsToPng(buffer, size);
    } finally {
      // Restore everything, regardless of encode outcome.
      this._selection.setHighlightVisible(true);
      for (const node of unculled) {
        node.frustumCulled = true;
      }
      this.renderer.setRenderTarget(prevTarget);
      this.scene.background = prevBackground;
      this.setTheme(options.liveIsDark);
      for (const child of rehide) {
        child.visible = true;
      }
      if (subjectGroup) {
        // Detach subjects so the caller can dispose them; drop the temp group.
        subjectGroup.clear();
        this.scene.remove(subjectGroup);
      }
      target.dispose();
    }

    return dataUrl;
  }

  animateToDirection(direction: Vector3, up: Vector3, forceOrtho = false): void {
    this._camera.animateToDirection(direction, up, forceOrtho);
  }

  orbitBy(azimuth: number, polar: number): void {
    this._camera.orbitBy(azimuth, polar);
  }

  rollBy(radians: number): void {
    this._camera.rollBy(radians);
  }

  setObjectMode(mode: ObjectMode): void {
    this._selection.setObjectMode(mode);
  }

  /** macOS bare-two-finger-swipe action (orbit or pan). */
  setTwoFingerGesture(gesture: TwoFingerGesture): void {
    this._controls.setTwoFingerGesture(gesture);
  }

  /**
   * Enable or disable pen-priority palm rejection ("wrist detection"). When
   * enabled (default) touch contacts from the hand resting on the glass are
   * swallowed while an Apple Pencil / stylus is in use, so the palm never
   * orbits or pinches the camera. See {@link PointerArbiter}.
   */
  setPalmRejectionEnabled(enabled: boolean): void {
    this._pointerArbiter.setEnabled(enabled);
  }

  /** Set the perspective field-of-view (degrees); applied live when perspective. */
  setFieldOfView(fov: number): void {
    this._camera.setPerspectiveFov(fov);
  }

  /** Cap the device-pixel-ratio the renderer draws at and repaint at the new size. */
  setPixelRatioCap(cap: number): void {
    this.pixelRatioCap = cap;
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, cap));
    const { clientWidth, clientHeight } = this.sizeOf(this.host);
    this.renderer.setSize(clientWidth, clientHeight);
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    cancelAnimationFrame(this.rafHandle);
    this.resizeObserver.disconnect();
    this._controls.dispose();
    this._grid.dispose();
    this._selection.dispose();
    this._pointerArbiter.dispose();
    this.gizmo.dispose();
    this.clearContent();
    this.controls.dispose();
    this.renderer.dispose();
    if (this.renderer.domElement.parentElement === this.host) {
      this.host.removeChild(this.renderer.domElement);
    }
  }

  // -------------------------------------------------------------------------
  // Render loop
  // -------------------------------------------------------------------------

  private tick = (): void => {
    if (this.disposed) {
      return;
    }
    this.rafHandle = requestAnimationFrame(this.tick);
    const now = performance.now();
    const dt = this.lastFrameTime === 0 ? 0 : Math.min(0.1, (now - this.lastFrameTime) / 1000);
    this.lastFrameTime = now;

    if (this._camera.isAnimating()) {
      this._camera.advance();
    } else {
      if (this._controls.hasAutoscroll()) {
        this._controls.applyAutoscrollZoom(dt);
      }
      this.controls.update();
      this._controls.applyOrbitInertia(dt);
    }

    this._grid.updateAdaptiveGrid();
    this._grid.updateGridFade();
    this._camera.updateNearFar();

    if (!this.gizmo.isDragging()) {
      this.gizmo.setCentroid(this._selection.computeSelectionCentroid());
    }
    this.gizmo.update();

    const dist = this.camera.position.distanceTo(this.axesGizmo.position);
    // opacity between 60 (alpha 0) and 140 (alpha 1)
    const opacity = Math.max(0, Math.min(1, (dist - 60) / 80));
    this.axesGizmo.traverse((c) => {
      const mesh = c as Mesh;
      if (mesh.material && (mesh.material as MeshBasicMaterial).transparent !== undefined) {
        (mesh.material as MeshBasicMaterial).opacity = opacity;
        mesh.visible = opacity > 0;
      }
    });

    this.renderer.render(this.scene, this.camera);
    this.publishCameraState();
    this.publishFps(now, dt);
  };

  private publishFps(now: number, dt: number): void {
    if (!this.fpsSink) {
      return;
    }
    if (dt > 0) {
      const instantFps = 1 / dt;
      const instantDelayMs = dt * 1000;
      this.smoothedFps =
        this.smoothedFps === 0 ? instantFps : 0.9 * this.smoothedFps + 0.1 * instantFps;
      this.smoothedDelayMs =
        this.smoothedDelayMs === 0
          ? instantDelayMs
          : 0.9 * this.smoothedDelayMs + 0.1 * instantDelayMs;
    }
    if (now - this.lastFpsPublishTime >= 500) {
      this.lastFpsPublishTime = now;
      this.fpsSink(Math.round(this.smoothedFps), Math.round(this.smoothedDelayMs * 10) / 10);
    }
  }

  private publishCameraState(): void {
    if (!this.cameraStateSink) {
      return;
    }
    const offset = this.camera.position.clone().sub(this.controls.target);
    if (offset.lengthSq() < 1e-6) {
      const DEFAULT_VIEW_DIR = new Vector3(1, -1, 0.8).normalize();
      offset.copy(DEFAULT_VIEW_DIR);
    }
    this.cameraStateSink(offset.normalize(), this.camera.up.clone().normalize(), this.camera.fov);
  }

  private handleResize(): void {
    const { clientWidth, clientHeight } = this.sizeOf(this.host);
    if (clientWidth === 0 || clientHeight === 0) {
      return;
    }
    this.camera.aspect = clientWidth / clientHeight;
    this.camera.updateProjectionMatrix();
    this.renderer.setSize(clientWidth, clientHeight);
    this.renderer.render(this.scene, this.camera);
  }

  private sizeOf(el: HTMLElement): { clientWidth: number; clientHeight: number } {
    return {
      clientWidth: Math.max(el.clientWidth, 1),
      clientHeight: Math.max(el.clientHeight, 1),
    };
  }
}

// Re-export public types so callers can import from './scene' directly.
export type { SceneGizmoHandlers, SceneSelectionHandlers, ViewerView };

// -----------------------------------------------------------------------------
// RGB axes gizmo
// -----------------------------------------------------------------------------

function buildAxesGizmo(length: number, thickness: number): Group {
  const group = new Group();
  group.renderOrder = 1;

  const halfT = thickness / 2;
  const originGeo = new BoxGeometry(thickness, thickness, thickness);
  const originMat = new MeshBasicMaterial({ color: 0xdddddd, transparent: true, opacity: 1 });
  const originMesh = new Mesh(originGeo, originMat);
  originMesh.position.set(halfT, halfT, halfT);
  group.add(originMesh);

  const axes: Array<{ color: number; axis: 'x' | 'y' | 'z' }> = [
    { color: 0xff3344, axis: 'x' },
    { color: 0x33dd55, axis: 'y' },
    { color: 0x4488ff, axis: 'z' },
  ];
  const rodLength = length - thickness;
  for (const { color, axis } of axes) {
    const dims: [number, number, number] =
      axis === 'x'
        ? [rodLength, thickness, thickness]
        : axis === 'y'
          ? [thickness, rodLength, thickness]
          : [thickness, thickness, rodLength];
    const geo = new BoxGeometry(...dims);
    const mat = new MeshBasicMaterial({ color, transparent: true, opacity: 1 });
    const mesh = new Mesh(geo, mat);
    mesh.position.set(halfT, halfT, halfT);
    mesh.position[axis] = thickness + rodLength / 2;
    mesh.renderOrder = 1;
    group.add(mesh);
  }
  return group;
}

/**
 * Encode an RGBA pixel buffer read from a WebGL render target into a PNG data
 * URL. WebGL returns rows bottom-up, so each row is copied into its mirrored
 * position to produce a correctly-oriented image.
 */
function encodePixelsToPng(buffer: Uint8Array, size: number): string | null {
  const canvas = document.createElement('canvas');
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext('2d');
  if (!ctx) {
    return null;
  }
  const image = ctx.createImageData(size, size);
  const rowBytes = size * 4;
  for (let y = 0; y < size; y++) {
    const srcStart = y * rowBytes;
    const dstStart = (size - 1 - y) * rowBytes;
    image.data.set(buffer.subarray(srcStart, srcStart + rowBytes), dstStart);
  }
  ctx.putImageData(image, 0, 0);
  return canvas.toDataURL('image/png');
}
