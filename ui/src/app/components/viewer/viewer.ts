import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  computed,
  effect,
  inject,
  input,
  output,
  signal,
  untracked,
  viewChild,
} from '@angular/core';
import { BufferAttribute, BufferGeometry, Matrix4, Mesh, MeshPhongMaterial, Vector3 } from 'three';
import { AppTheme } from '../../services/app-theme';
import { GcodePreview, ROLE_LABELS, scalarChannelFor } from '../../services/gcode-preview';
import { ObjectTracker } from '../../services/object-tracker';
import { PrintArea } from '../../services/print-area';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { SceneCommand } from '../../services/scene-command/scene-command';
import { SceneEngine } from '../../services/scene-engine';
import { ViewerControl } from '../../services/viewer-control';
import {
  pixelRatioCapFor,
  resolveAntialias,
  type Antialiasing,
  type SliceThumbnailCapture,
  type SliceThumbnailRequest,
  type ThumbnailColorMode,
  type ThumbnailView,
} from '../../services/viewer-control';

import { GcodeHoverProbe, type GcodeHoverHit } from './gcode-hover';
import { GcodeOrchestrator } from './gcode-orchestrator';
import { preferredHoverPlacement } from './hover-placement';
import type { GizmoDelta } from './gizmo';
import { ViewerScene } from './scene';
import type { ViewerView } from './scene';
import { applyFloating, type FloatingPlacement } from '@coldcrabby/ui';

export type ViewerMode = 'model' | 'gcode';

/** Input accepted by the model input. */
export type ModelSource = string | URL | File | Blob | ArrayBuffer;

/**
 * Base model colour per theme. A single coherent neutral graphite so the mesh
 * reads as the same "grey plastic" object in both themes. The light-mode shade
 * is lighter so a mid-grey object doesn't read as heavy/dark against the near-
 * white background; the dark-mode shade is a hair deeper so it doesn't glow
 * against the near-black background. Both are near-neutral (minimal blue).
 */
const MODEL_COLOR_DARK = 0xbcc0c6;
const MODEL_COLOR_LIGHT = 0xccd0d4;

/**
 * Triangles per frame full detail may cost *while the user is interacting*.
 *
 * Under this, full detail is kept on permanently — including during orbit — so
 * ordinary plates never visibly change as you move. Geometry is one merged
 * buffer per role, so every segment in the visible layer range is submitted
 * wherever the camera points; zoom cannot lower this, but the layer slider can.
 */
const INTERACTIVE_TRIANGLE_BUDGET = 12_000_000;

/**
 * Triangles per frame full detail may cost once the view has *settled*.
 *
 * Far larger than the interactive budget because on-demand rendering makes a
 * still view cost exactly one frame: a heavy frame is a single settle-in, not a
 * sustained frame rate. This is what lets a million-segment plate still be
 * inspected at full quality.
 */
const SETTLED_TRIANGLE_BUDGET = 120_000_000;

/**
 * Measured frame time (ms) above which full detail is judged unaffordable on
 * this machine and auto mode stops promoting to it.
 *
 * This is the actual hardware autodetection: rather than guessing from a GPU
 * name (routinely masked, and no guide to real throughput), auto mode promotes
 * once, measures, and believes the result.
 */
const SETTLE_FRAME_BUDGET_MS = 400;

/** Resolve the model base colour for the active colour scheme. */
function modelColor(isDark: boolean): number {
  return isDark ? MODEL_COLOR_DARK : MODEL_COLOR_LIGHT;
}

/**
 * Fixed camera directions (world Z-up, target → camera) and up vectors for
 * each thumbnail preset. Directions are normalised on use. A slight elevation
 * on the side views reads as a product shot rather than a flat elevation.
 */
const THUMBNAIL_VIEW_POSES: Record<ThumbnailView, { dir: Vector3; up: Vector3 }> = {
  isometric: { dir: new Vector3(1, -1, 0.8), up: new Vector3(0, 0, 1) },
  front: { dir: new Vector3(0, -1, 0.32), up: new Vector3(0, 0, 1) },
  rear: { dir: new Vector3(0, 1, 0.32), up: new Vector3(0, 0, 1) },
  left: { dir: new Vector3(-1, 0, 0.32), up: new Vector3(0, 0, 1) },
  right: { dir: new Vector3(1, 0, 0.32), up: new Vector3(0, 0, 1) },
  // Pure top-down: nudge off the pole so the view axis isn't parallel to up,
  // and orient so the bed's far edge (+Y) points to the top of the image.
  top: { dir: new Vector3(0, 0.001, 1), up: new Vector3(0, -1, 0) },
};

/** Solid studio background behind the model in each thumbnail theme. */
const THUMBNAIL_BG_LIGHT = 0xe9ebee;
const THUMBNAIL_BG_DARK = 0x212327;

/** How long the outbound thumbnail card lingers on screen (ms). */
const THUMBNAIL_SHUTTER_MS = 460;
const THUMBNAIL_POLAROID_MS = 3200;

/**
 * Single-component 3D viewer for both raw meshes and sliced G-code.
 *
 * The viewer is the only entry point for visualization. It owns the
 * Three.js scene, switches between the two render modes without
 * re-initializing WebGL, and drives the streaming G-code pipeline.
 *
 * Usage:
 * ```html
 * <nexus-viewer [model]="stlFileOrUrl" mode="model"></nexus-viewer>
 * <nexus-viewer mode="gcode"></nexus-viewer>
 * ```
 */
@Component({
  selector: 'nexus-viewer',
  standalone: true,
  templateUrl: './viewer.html',
  styleUrl: './viewer.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class Viewer {
  readonly mode = input<ViewerMode>('model');
  readonly model = input<ModelSource | null>(null);
  /**
   * Uploaded-file id backing {@link model}, stamped onto the scene object so
   * a slice can resolve it back to the right bytes. Objects added later carry
   * their own id from wherever they were added.
   */
  readonly modelSourceId = input<string | null>(null);
  readonly showTravel = input(false);

  readonly loadComplete = output<{ mode: ViewerMode; segments: number }>();
  readonly loadError = output<{ mode: ViewerMode; error: unknown }>();

  private readonly hostRef = viewChild.required<ElementRef<HTMLElement>>('host');
  /** The G-code inspector tooltip element (present only while hovering). */
  private readonly gcodeTooltipRef = viewChild<ElementRef<HTMLElement>>('gcodeTooltip');
  private readonly viewerControl = inject(ViewerControl);
  private readonly printArea = inject(PrintArea);
  private readonly objectTracker = inject(ObjectTracker);
  private readonly sceneEngine = inject(SceneEngine);
  private readonly sceneCommand = inject(SceneCommand);
  private readonly gcodePreview = inject(GcodePreview);
  private readonly activeSelection = inject(ActiveSelection);
  private readonly appTheme = inject(AppTheme);
  private readonly destroyRef = inject(DestroyRef);

  /** Current loading status for the optional overlay. */
  readonly status = signal<'idle' | 'loading' | 'streaming' | 'ready' | 'error'>('idle');
  /** Smoothed frames-per-second reported by the render loop. */
  readonly fps = signal(0);
  /** Smoothed average frame delay in milliseconds. */
  readonly frameDelayMs = signal(0);
  /** Whether the view has stopped changing (drives the detail promotion). */
  private viewSettled = false;
  /** Cleared when a promoted full-detail frame proved too slow on this machine. */
  private fullDetailAffordable = true;
  /** Set while waiting to measure the cost of a just-promoted frame. */
  private probingDetail = false;
  /**
   * End-to-end wall time of the last WASM mesh round-trip, measured from
   * the moment the bytes are handed to `addMesh` to the moment the
   * `RenderBuffer` is fully copied out of WASM memory. `null` until the
   * first model has been loaded through the engine.
   */
  readonly wasmRoundtripMs = signal<number | null>(null);
  /** WASM-side parse time (`addMesh`) of the last model load. */
  readonly wasmParseMs = signal<number | null>(null);
  /** WASM-side render-buffer extraction time (`getRenderBuffer`) of the last load. */
  readonly wasmRenderBufMs = signal<number | null>(null);
  /** Last-op rolling stats from the scene engine, surfaced in the overlay. */
  readonly opStats = computed(() => this.sceneEngine.opStats());
  /** User preference: show or hide scene telemetry chips. */
  readonly statsVisible = this.viewerControl.statsVisible;
  private readonly progressSegments = signal(0);
  private readonly errorMessage = signal<string>('');

  /** Inspector readout for the extrusion under the cursor (G-code scalar views). */
  readonly hoverInfo = this.gcodePreview.hoverInfo;
  /** Role display labels for the hover tooltip. */
  protected readonly roleLabels = ROLE_LABELS;
  /** One-shot white shutter pulse shown while capturing a slice thumbnail. */
  readonly thumbnailShutterActive = signal(false);
  /** Data-URL preview card for the outbound thumbnail animation. */
  readonly thumbnailPolaroidImage = signal<string | null>(null);
  /** Drives the "slide to top" thumbnail card animation class. */
  readonly thumbnailPolaroidAnimating = signal(false);

  private scene: ViewerScene | null = null;
  /**
   * View value most recently pushed *from* the camera into
   * `viewerControl.view` (a cube snap engaging ortho, or a breakout restoring
   * the previous preset). Armed only when the write actually changes the
   * signal, so the effect it triggers can skip echoing it straight back into the
   * camera. Cleared by that skip.
   */
  private cameraOriginatedView: ViewerView | null = null;
  private gcode: GcodeOrchestrator | null = null;
  private gcodeHover: GcodeHoverProbe | null = null;
  private currentAbort: AbortController | null = null;
  private loadToken = 0;
  /** SceneObject ids registered for the currently-loaded source. */
  private trackedObjectIds: string[] = [];
  /** Live mapping from WASM scene id to the Three.js mesh that mirrors it. */
  private readonly wasmMeshes = new Map<bigint, Mesh>();
  /**
   * The model source that is currently loaded into the WASM scene engine.
   * Used to detect whether a source change is a new model (full teardown)
   * or just a mode switch (WASM state preserved, transforms kept).
   */
  private activeModelSource: ModelSource | null = null;
  private readonly tmpMatrix = new Matrix4();
  /**
   * Currently selected WASM object ids (as bigint), kept in sync with the
   * legacy scene's string-id selection set so highlight + drag work.
   */
  private selectedWasmIds: bigint[] = [];
  /**
   * Per-axis displacement (mm) already pushed to the engine for the
   * in-flight drag, indexed by WASM id. (Currently unused — the gizmo
   * already reports per-frame deltas — retained as a stub in case a
   * cumulative-protocol drag handler is reintroduced later.)
   */
  private dragApplied = new Map<bigint, { dx: number; dy: number }>();

  /**
   * Last anti-aliasing mode applied to the live scene. `null` until the first
   * effect run seeds it; used to detect real changes (which force a rebuild).
   */
  private lastAntialiasing: Antialiasing | null = null;

  /** Cursor anchor for the G-code tooltip, exposed to Floating UI as a virtual element. */
  private readonly gcodeCursor = { x: 0, y: 0 };
  private readonly gcodeCursorAnchor = {
    getBoundingClientRect: () => {
      const { x, y } = this.gcodeCursor;
      return { x, y, top: y, left: x, right: x, bottom: y, width: 0, height: 0 } as DOMRect;
    },
  };
  private stopGcodeFloating: (() => void) | null = null;
  /** Preferred side the G-code tooltip is currently floated to; drives re-anchoring when the input hand/side changes. */
  private gcodeFloatingPlacement: FloatingPlacement | null = null;
  private shutterTimer: ReturnType<typeof setTimeout> | null = null;
  private polaroidTimer: ReturnType<typeof setTimeout> | null = null;
  /**
   * Base64 PNG of the most recently captured slice thumbnail. The capture FX
   * (shutter flash + polaroid) only plays when a fresh capture differs from
   * this, so re-slicing an unchanged scene doesn't re-fling an identical
   * preview. Reliable within one client: the same model + thumbnail settings
   * render byte-identical from the fixed capture viewpoint (the cross-client
   * byte variance that keeps the PNG out of the server cache key doesn't occur
   * when the *same* renderer re-shoots the *same* scene).
   */
  private lastThumbnailImage: string | null = null;
  private readonly captureSliceThumbnailSink = (request: SliceThumbnailRequest) =>
    this.captureSliceThumbnail(request);

  constructor() {
    afterNextRender(() => this.initScene());

    this.destroyRef.onDestroy(() => {
      this.cancelInFlightLoad();
      this.gcodeHover?.dispose();
      this.gcodeHover = null;
      this.gcode?.dispose();
      this.gcode = null;
      this.scene?.dispose();
      this.scene = null;
      this.viewerControl.orbitSink = null;
      if (this.viewerControl.sliceThumbnailCaptureSink === this.captureSliceThumbnailSink) {
        this.viewerControl.sliceThumbnailCaptureSink = null;
      }
      this.stopGcodeFloating?.();
      this.stopGcodeFloating = null;
      this.gcodeFloatingPlacement = null;
      this.clearThumbnailFxTimers();
      this.thumbnailShutterActive.set(false);
      this.thumbnailPolaroidAnimating.set(false);
      this.thumbnailPolaroidImage.set(null);
    });

    // Position the G-code inspector tooltip with Floating UI, anchored to a
    // virtual element at the cursor so it flips/shifts to stay on-screen near
    // the viewport edges instead of clipping. The preferred side adapts to the
    // input: below-right for a mouse, but opposite the hand for a pen (from its
    // tilt) and above the contact for touch, so the hand never covers it.
    effect(() => {
      const info = this.gcodePreview.hoverInfo();
      const el = this.gcodeTooltipRef()?.nativeElement;
      if (info && el) {
        this.gcodeCursor.x = info.clientX;
        this.gcodeCursor.y = info.clientY;
        const placement = preferredHoverPlacement(info);
        // Placement is fixed when applyFloating is created, so re-anchor when
        // the desired side changes (e.g. the user switches from mouse to pen,
        // or tilts the pen across an axis).
        if (!this.stopGcodeFloating || this.gcodeFloatingPlacement !== placement) {
          this.stopGcodeFloating?.();
          this.gcodeFloatingPlacement = placement;
          this.stopGcodeFloating = applyFloating(this.gcodeCursorAnchor, el, {
            placement,
            strategy: 'fixed',
            offset: 14,
            padding: 8,
            hideWhenDetached: false,
          });
        }
      } else if (this.stopGcodeFloating) {
        this.stopGcodeFloating();
        this.stopGcodeFloating = null;
        this.gcodeFloatingPlacement = null;
      }
    });

    // React to input changes — single effect handles mode + source switching.
    effect(() => {
      const mode = this.mode();
      const model = this.model();

      if (!this.scene) {
        return;
      }
      this.applySource(mode, model);
    });

    // React to view-preset changes from the toolbar. Changes the *camera*
    // originated (a cube snap engaging ortho, or a breakout restoring the
    // previous preset) are echoed into the same signal so the toolbar button
    // always matches the screen; those must not be routed back into the camera,
    // which would cancel the in-flight snap and its detent.
    effect(() => {
      const view = this.viewerControl.view();
      if (view === this.cameraOriginatedView) {
        this.cameraOriginatedView = null;
        return;
      }
      this.scene?.setView(view);
    });

    // React to object-mode (gizmo) changes from the toolbar.
    effect(() => {
      const mode = this.viewerControl.objectMode();
      this.scene?.setObjectMode(mode);
    });

    // React to the trackpad two-finger gesture preference (Shapr3D-style).
    effect(() => {
      const gesture = this.viewerControl.trackpadTwoFingerGesture();
      this.scene?.setTwoFingerGesture(gesture);
    });

    // React to the palm-rejection ("wrist detection") preference for stylus use.
    effect(() => {
      const enabled = this.viewerControl.palmRejection();
      this.scene?.setPalmRejectionEnabled(enabled);
    });

    // React to field-of-view changes from the 3D-view settings.
    effect(() => {
      const fov = this.viewerControl.fieldOfView();
      this.scene?.setFieldOfView(fov);
    });

    // React to render-resolution (pixel-ratio cap) changes.
    effect(() => {
      const quality = this.viewerControl.renderQuality();
      this.scene?.setPixelRatioCap(pixelRatioCapFor(quality));
    });

    // React to anti-aliasing changes. MSAA is a WebGLRenderer construction
    // option, so it cannot be toggled on a live context — rebuild the scene
    // (the WASM engine keeps object state, so the model is restored intact).
    effect(() => {
      const mode = this.viewerControl.antialiasing();
      if (this.lastAntialiasing === null) {
        this.lastAntialiasing = mode;
        return;
      }
      if (mode === this.lastAntialiasing) {
        return;
      }
      this.lastAntialiasing = mode;
      if (this.scene) {
        this.rebuildScene();
      }
    });

    // React to colour-scheme changes: retune the scene lighting and repaint
    // the model meshes so the object keeps good contrast on both themes.
    effect(() => {
      const isDark = this.appTheme.isDarkMode();
      this.scene?.setTheme(isDark);
      const color = resolveModelColor(
        isDark,
        this.viewerControl.useFilamentColor(),
        this.activeSelection.filament()?.color,
      );
      for (const mesh of this.wasmMeshes.values()) {
        (mesh.material as MeshPhongMaterial).color.setHex(color);
      }
    });

    // React to reset requests from the toolbar.
    effect(() => {
      const tick = this.viewerControl.resetTick();
      // Skip the very first emission so we don't redundantly reset on init.
      if (tick === 0) {
        return;
      }
      this.scene?.resetView();
    });

    // React to direction-look requests (e.g. from the viewport-cube gizmo).
    effect(() => {
      const req = this.viewerControl.lookRequest();
      if (!req) {
        return;
      }
      this.scene?.animateToDirection(req.direction, req.up, req.autoOrtho);
    });
    // React to roll requests (viewport-cube roll buttons).
    effect(() => {
      const req = this.viewerControl.rollRequest();
      if (!req) {
        return;
      }
      this.scene?.rollBy(req.radians);
    });

    // Mirror the print-area configuration into the scene so the bed grid
    // tracks any settings/UI changes (dimensions or movable-area offset).
    effect(() => {
      const config = this.printArea.config();
      this.scene?.setPrintArea(config);
    });

    // Mirror the application's selection state into the scene so meshes get
    // their highlight as soon as the service signal flips (whether the flip
    // came from a viewer click or from external UI).
    effect(() => {
      const ids = this.printArea.selectedIds();
      this.scene?.setSelectedIds(ids);
    });

    // Mirror tracked-object transforms onto the corresponding mesh nodes.
    // Reading every SceneObject's `transform` signal in this effect makes
    // it depend on each object's position/rotation/scale, so any update
    // (manual API call, drag, future gizmo) re-runs and pushes through.
    //
    // DISABLED during migration to the WASM scene engine. Object transforms
    // now flow through `wasmMeshes` (see effect below); the legacy tracker
    // path is kept dormant for the eventual selection / gizmo work.
    // effect(() => {
    //   const objects = this.objectTracker.objects();
    //   const scene = this.scene;
    //   if (!scene) {
    //     return;
    //   }
    //   for (const obj of objects) {
    //     scene.setObjectTransform(obj.id, obj.transform());
    //   }
    // });

    // Push the WASM scene-engine transform onto each mirrored Three.js mesh
    // every time the snapshot changes. This is the equivalent of the legacy
    // ObjectTracker mirror above — only the source of truth has moved into
    // Rust. Matrices are read column-major (matches glam) and applied with
    // `matrixAutoUpdate = false` so Three.js does not overwrite them.
    effect(() => {
      const objects = this.sceneEngine.objects();
      if (this.mode() !== 'model' || !this.scene) {
        return;
      }
      // Reconcile membership first: an object added or removed by anyone —
      // the add-object button, a duplicate, an undo — must show up here
      // without the viewer being told about it.
      const mirrored = untracked(() => this.wasmMeshes.size);
      if (objects.length !== mirrored || objects.some((o) => !this.wasmMeshes.has(o.id))) {
        untracked(() => this.syncWasmMeshes());
      }
      for (const obj of objects) {
        const mesh = this.wasmMeshes.get(obj.id);
        if (!mesh) {
          continue;
        }
        const m = this.sceneEngine.getMatrix(obj.id);
        this.tmpMatrix.fromArray(m);
        mesh.matrix.copy(this.tmpMatrix);
        mesh.matrixWorldNeedsUpdate = true;
      }
      this.scene?.invalidate();
    });

    // Mirror externally-driven selection (the objects panel) into the 3D
    // scene. `viewerControl.selectedObjectIds` is the shared selection state;
    // without this the viewer only ever *wrote* it, so clicking a row in the
    // panel highlighted nothing and left the gizmo unattached.
    effect(() => {
      const ids = this.viewerControl.selectedObjectIds();
      if (!this.scene) {
        return;
      }
      untracked(() => {
        if (sameIds(ids, this.selectedWasmIds)) {
          return;
        }
        // Only keep ids the viewer actually has a mesh for; a panel row for an
        // object mid-teardown must not resurrect a dead selectable.
        this.selectedWasmIds = ids.filter((id) => this.wasmMeshes.has(id));
        this.scene?.setSelectedIds(new Set(this.selectedWasmIds.map(String)));
        this.scene?.invalidate();
      });
    });

    // React to layer-range changes from the GcodePreviewService.
    effect(() => {
      const min = this.gcodePreview.layerMin();
      const max = this.gcodePreview.layerMax();
      this.gcode?.showRange(min, max);
      // The layer range changes what a frame costs, so it can earn (or lose)
      // full detail independently of the camera.
      this.refreshGcodeDetail();
      this.scene?.invalidate();
    });

    // React to nozzle-progress changes.
    effect(() => {
      const progress = this.gcodePreview.segmentProgress();
      const max = this.gcodePreview.layerMax();
      this.gcode?.applyProgress(max, progress);
      this.refreshGcodeDetail();
      this.scene?.invalidate();
    });

    // React to role visibility changes.
    effect(() => {
      const hidden = this.gcodePreview.hiddenRoles();
      this.gcode?.applyHiddenRoles(hidden);
      this.scene?.invalidate();
    });

    // React to the preview-detail preference.
    effect(() => {
      this.viewerControl.previewDetail();
      this.refreshGcodeDetail();
    });

    // React to theme, view-mode, scalar-range, fan-selection, or legend
    // hover-band changes — recolor all layers in place without rebuilding.
    effect(() => {
      const mode = this.gcodePreview.effectiveViewMode();
      const colors = this.gcodePreview.roleColors();
      const range = this.gcodePreview.activeRange();
      const fan = this.gcodePreview.selectedFan();
      const band = this.gcodePreview.hoverBand();
      this.gcode?.applyView(colors, scalarChannelFor(mode), range, fan, band);
      this.scene?.invalidate();
    });

    // The hover-inspect probe is only meaningful in the G-code scalar views.
    effect(() => {
      const active =
        this.mode() === 'gcode' && scalarChannelFor(this.gcodePreview.effectiveViewMode()) !== null;
      this.gcodeHover?.setEnabled(active);
      if (!active) {
        this.gcodePreview.setHoverInfo(null);
      }
    });

    // Build (or rebuild) the layer graph when the parsed handle becomes
    // available or is replaced. This is intentionally separate from the
    // `applySource` effect so that layer-range / role / progress slider ticks
    // (which `applySource` reads via `untracked`) do not redundantly tear
    // down and rebuild every layer group.
    effect(() => {
      const handle = this.gcodePreview.gcodeHandle();
      if (!handle || untracked(() => this.mode()) !== 'gcode' || !this.scene) {
        return;
      }
      this.startGcodeFromHandle();
    });
  }

  // ---------------------------------------------------------------------------
  // Selection / gizmo handlers
  //
  // The legacy `ViewerScene` raycast pointer plumbing calls `handleSelect` /
  // `handleClearSelection`; gizmo-driven object manipulation goes through
  // `handleGizmoDelta` / `handleGizmoEnd` / `handleFacePicked` which
  // dispatch one WASM op per selected object id.
  // ---------------------------------------------------------------------------

  /**
   * Map a hover hit to the active channel value and publish it for the
   * inspector tooltip + legend tick, or clear the readout when nothing
   * extrudable is under the cursor.
   */
  /**
   * Pick the G-code detail level from the current view and layer range.
   *
   * Full detail requires both that the user is close enough for the rounding to
   * read *and* that what is on screen fits the triangle budget — the second
   * condition is the one that matters on big plates, where a merged buffer
   * submits every visible segment regardless of where the camera looks.
   */
  private refreshGcodeDetail(): void {
    const gcode = this.gcode;
    if (!gcode) {
      return;
    }
    const detail = this.resolveGcodeDetail(gcode);
    if (gcode.setDetail(detail)) {
      // Remember to check what a promotion actually cost.
      if (detail === 'high') {
        this.probingDetail = true;
      }
      this.scene?.invalidate();
    }
  }

  /** Apply the user's preference, falling back to the adaptive policy. */
  private resolveGcodeDetail(gcode: GcodeOrchestrator): 'high' | 'low' {
    switch (this.viewerControl.previewDetail()) {
      case 'performance':
        return 'low';
      case 'quality':
        return 'high';
      default:
        break;
    }
    // Cheap enough to keep full detail on permanently — no change while moving.
    if (gcode.canAffordHighDetail(INTERACTIVE_TRIANGLE_BUDGET)) {
      return 'high';
    }
    // Heavy plate: cheap beads while the user is moving, full detail the moment
    // they stop — which is when they are actually evaluating the result.
    if (!this.viewSettled || !this.fullDetailAffordable) {
      return 'low';
    }
    return gcode.canAffordHighDetail(SETTLED_TRIANGLE_BUDGET) ? 'high' : 'low';
  }

  private onGcodeHover(hit: GcodeHoverHit | null): void {
    if (!hit) {
      this.gcodePreview.setHoverInfo(null);
      return;
    }
    const mode = this.gcodePreview.effectiveViewMode();
    const channel = scalarChannelFor(mode);
    if (!channel) {
      this.gcodePreview.setHoverInfo(null);
      return;
    }

    const rs = hit.ref.roleSegments;
    const i = hit.instanceId;
    // Role buffers span every layer, so the layer has to be resolved from the
    // instance index rather than read off the mesh.
    const location = hit.ref.resolve(i);
    // The upper bound is a draw-range prefix, which the raycast already
    // respects, but the lower bound lives in the vertex shader — invisible to
    // a raycast. Reject hits below it so "single layer" mode can't report a
    // segment the user cannot see.
    if (location.layerIndex < this.gcodePreview.layerMin()) {
      this.gcodePreview.setHoverInfo(null);
      return;
    }
    const width = rs.widths?.[i] ?? 0;
    const height = rs.heights?.[i] ?? 0;
    const speed = rs.speeds?.[i] ?? 0;
    const accel = rs.accels?.[i] ?? 0;
    const value =
      channel.scope === 'segment'
        ? channel.extract(width, height, speed, accel)
        : channel.extractLayer(location.meta, this.gcodePreview.selectedFan());
    if (value === null) {
      this.gcodePreview.setHoverInfo(null);
      return;
    }

    const range = this.gcodePreview.activeRange();
    const span = range.max - range.min;
    const t = span > 0 ? Math.min(1, Math.max(0, (value - range.min) / span)) : 0.5;
    this.gcodePreview.setHoverInfo({
      channelId: mode,
      value,
      valueLabel: channel.format(value),
      role: rs.role,
      layerIndex: location.layerIndex,
      z: location.z,
      width,
      height,
      speed,
      t,
      clientX: hit.clientX,
      clientY: hit.clientY,
      pointerType: hit.pointerType,
      tiltX: hit.tiltX,
      tiltY: hit.tiltY,
    });
  }

  /**
   * Mirror a camera-originated projection change into the toolbar's `view`
   * signal, so the projection button's icon, tooltip and next press always match
   * what is on screen. Arms {@link cameraOriginatedView} only when the signal
   * actually changes — that is exactly when the effect will fire and needs to
   * skip pushing the value back into the camera.
   */
  private syncViewFromCamera(view: ViewerView): void {
    if (this.viewerControl.view() === view) {
      return;
    }
    this.cameraOriginatedView = view;
    this.viewerControl.view.set(view);
  }

  private handleSelect(stringId: string, additive: boolean): void {
    const id = parseWasmId(stringId);
    if (id === null) {
      return;
    }
    const selected = this.selectedWasmIds.includes(id);
    if (!additive) {
      // Plain click replaces the selection, the way every other editor
      // behaves — clicking objects in turn should walk the selection, not
      // accumulate one. Clicking the sole selected object deselects it.
      this.selectedWasmIds = selected && this.selectedWasmIds.length === 1 ? [] : [id];
    } else if (selected) {
      this.selectedWasmIds = this.selectedWasmIds.filter((existing) => existing !== id);
    } else {
      this.selectedWasmIds = [...this.selectedWasmIds, id];
    }
    this.scene?.setSelectedIds(new Set(this.selectedWasmIds.map(String)));
    this.viewerControl.selectedObjectIds.set(this.selectedWasmIds);
  }

  private handleClearSelection(): void {
    this.selectedWasmIds = [];
    this.scene?.setSelectedIds(new Set());
    this.viewerControl.selectedObjectIds.set([]);
  }

  /** Translate / rotate / scale a delta onto every currently-selected object. */
  private handleGizmoDelta(stringIds: readonly string[], delta: GizmoDelta): void {
    for (const stringId of stringIds) {
      const id = parseWasmId(stringId);
      if (id === null) {
        continue;
      }
      switch (delta.kind) {
        case 'translate':
          this.sceneCommand.apply({
            op: 'Translate',
            args: { id, delta: delta.delta },
          });
          break;
        case 'rotate':
          this.sceneCommand.apply({
            op: 'Rotate',
            args: { id, axis: delta.axis, degrees: delta.degrees },
          });
          break;
        case 'scale':
          this.sceneCommand.apply({
            op: 'Scale',
            args: { id, factors: delta.factors },
          });
          break;
      }
    }
  }

  /** Flush any in-progress gesture so the history entry is committed. */
  private handleGizmoEnd(): void {
    // When gravity is enabled, drop every selected object to the floor before
    // committing the history entry. This keeps the drop part of the same
    // gesture so undo reverts the entire move + drop as one unit.
    if (this.viewerControl.gravityEnabled()) {
      for (const id of this.selectedWasmIds) {
        this.sceneCommand.apply({ op: 'DropToFloor', args: { id } });
      }
    }
    this.sceneCommand.flush();
  }

  /**
   * Pull-to-floor: align the picked face to Z=0. Stays in pull-to-floor
   * mode so the user can pick another face on another object without
   * having to re-enter the mode. Selection is left untouched — picking a
   * face is a manipulation gesture, not a selection gesture.
   */
  private handleFacePicked(stringId: string, faceIndex: number): void {
    const id = parseWasmId(stringId);
    if (id === null) {
      return;
    }
    this.sceneCommand.apply({
      op: 'PlaceFaceOnFloor',
      args: { id, face_index: faceIndex },
    });
    this.sceneCommand.flush();
  }

  statusLabel(): string {
    switch (this.status()) {
      case 'loading':
        return 'Loading…';
      case 'streaming':
        return `Streaming… ${this.progressSegments().toLocaleString()} segments`;
      case 'ready':
        return `Ready — ${this.progressSegments().toLocaleString()} segments`;
      case 'error': {
        const detail = this.errorMessage();
        return detail ? `Failed to load — ${detail}` : 'Failed to load';
      }
      default:
        return '';
    }
  }

  /**
   * Tear down and recreate the Three.js scene, preserving the WASM scene
   * engine's object state so the loaded model is restored. Used when a
   * construction-only renderer option (anti-aliasing) changes at runtime.
   */
  private rebuildScene(): void {
    if (!this.scene) {
      return;
    }
    this.cancelInFlightLoad();
    this.gcodeHover?.dispose();
    this.gcodeHover = null;
    this.gcode?.dispose();
    this.gcode = null;
    this.scene.dispose();
    this.scene = null;
    this.wasmMeshes.clear();
    this.initScene();
  }

  private initScene(): void {
    const host = this.hostRef().nativeElement;
    this.scene = new ViewerScene(host, this.printArea.config(), {
      antialias: resolveAntialias(this.viewerControl.antialiasing()),
      pixelRatioCap: pixelRatioCapFor(this.viewerControl.renderQuality()),
      fieldOfView: this.viewerControl.fieldOfView(),
    });
    this.lastAntialiasing = this.viewerControl.antialiasing();
    // Mirror the live camera direction/up into ViewerControl so external
    // overlays (the viewport-cube gizmo) can read it without going through
    // Angular's change-detection.
    const state = this.viewerControl.cameraState;
    this.scene.cameraStateSink = (dir, up, fov) => {
      state.direction.copy(dir);
      state.up.copy(up);
      state.fov = fov;
    };
    this.scene.fpsSink = (fps, delayMs) => {
      this.fps.set(fps);
      this.frameDelayMs.set(delayMs);
    };
    // Pick the geometry detail from the camera: full detail only when the user
    // is close enough for the rounding to be visible *and* the plate is small
    // enough to afford it at all.
    this.scene.lodSink = ({ settled, lastFrameMs }) => {
      this.viewSettled = settled;
      // A promotion that turned out to be far too slow demotes permanently for
      // this model, so the user is not stuck with a lurching viewport.
      if (this.probingDetail) {
        this.probingDetail = false;
        if (lastFrameMs > SETTLE_FRAME_BUDGET_MS) {
          this.fullDetailAffordable = false;
        }
      }
      this.refreshGcodeDetail();
    };
    // Allow external gizmos (viewport-cube drag) to orbit the main camera.
    this.viewerControl.orbitSink = (azimuth, polar) => this.scene?.orbitBy(azimuth, polar);
    this.viewerControl.sliceThumbnailCaptureSink = this.captureSliceThumbnailSink;
    // Bridge raycast hits / gizmo gestures from the scene into the WASM
    // scene engine. Selection is stored locally; object manipulation is
    // driven by the gizmo (translate / rotate / scale) and pull-to-floor.
    this.scene.selectionHandlers = {
      select: (id, additive) => this.handleSelect(id, additive),
      clearSelection: () => this.handleClearSelection(),
    };
    this.scene.gizmoHandlers = {
      delta: (ids, delta) => this.handleGizmoDelta(ids, delta),
      end: () => this.handleGizmoEnd(),
      facePicked: (objectId, faceIndex) => this.handleFacePicked(objectId, faceIndex),
    };
    // Apply the current toolbar selections so the scene starts in sync with
    // whatever view / object mode the user already had selected.
    this.scene.setObjectMode(this.viewerControl.objectMode());
    this.scene.setView(this.viewerControl.view());
    // Keep the toolbar's projection button honest when the viewport cube (not
    // the toolbar) changes the projection.
    this.scene.setViewChangeSink((view) => this.syncViewFromCamera(view));
    this.scene.setTwoFingerGesture(this.viewerControl.trackpadTwoFingerGesture());
    this.scene.setPalmRejectionEnabled(this.viewerControl.palmRejection());
    this.scene.setTheme(this.appTheme.isDarkMode());
    this.gcode = new GcodeOrchestrator(this.scene.contentRoot);
    // Hover-inspect probe for the G-code scalar views: raycasts the visible
    // layer meshes and reports the extrusion value under the cursor.
    this.gcodeHover = new GcodeHoverProbe(
      this.scene.renderer.domElement,
      this.scene.camera,
      () => this.gcode?.hoverableMeshes() ?? [],
      (hit) => this.onGcodeHover(hit),
    );
    this.gcodeHover.setEnabled(
      this.mode() === 'gcode' && scalarChannelFor(this.gcodePreview.effectiveViewMode()) !== null,
    );
    // Seed the bed grid from the current print-area configuration.
    this.scene.setPrintArea(this.printArea.config());
    // Trigger initial source application now that the scene exists.
    this.applySource(this.mode(), this.model());
  }

  private applySource(mode: ViewerMode, model: ModelSource | null): void {
    const scene = this.scene;
    if (!scene) {
      return;
    }
    this.cancelInFlightLoad();

    const modelChanged = model !== this.activeModelSource;

    // When the viewer is re-created after navigating away and back to the same
    // workplate (e.g. dipping into Settings and returning), `activeModelSource`
    // resets to null so `modelChanged` is true — yet the singleton scene engine
    // still holds this plate's objects. Recognise that by matching the model's
    // source id against the engine, so we adopt the existing objects (keeping
    // their ids — hence the undo/redo history — and their transforms) instead
    // of evicting and re-parsing them.
    const sourceId = untracked(() => this.modelSourceId());
    const reopeningSamePlate =
      modelChanged &&
      model !== null &&
      sourceId != null &&
      untracked(() => this.sceneEngine.objects()).some((o) => o.source_id === sourceId);

    // Always tear down the G-code orchestrator and clear Three.js content —
    // the display layer is rebuilt for every mode/source transition.
    this.gcode?.dispose();
    scene.clearContent();
    this.progressSegments.set(0);
    this.errorMessage.set('');

    if (modelChanged && !reopeningSamePlate) {
      // New/different model source — full teardown of WASM engine objects so
      // ids do not accumulate and the old mesh's transforms are discarded
      // cleanly.
      for (const id of this.trackedObjectIds) {
        this.printArea.forgetObject(id);
        this.objectTracker.remove(id);
      }
      this.trackedObjectIds = [];
      for (const id of this.wasmMeshes.keys()) {
        this.scene?.unregisterSelectable(String(id));
      }
      this.wasmMeshes.clear();
      // Evict *every* object from the singleton scene engine — not only the
      // ones this component mirrored. Navigating between workplates destroys
      // the viewer component but not the WASM engine, so a previous plate's
      // object can survive in the engine even though this freshly-created
      // component never tracked it (its `wasmMeshes` map starts empty). Left
      // behind, that stale mesh would be sliced into the new plate and skew
      // its thumbnail. Read untracked so this teardown never subscribes the
      // enclosing effect to the object list it is about to mutate.
      for (const obj of untracked(() => this.sceneEngine.objects())) {
        try {
          this.sceneEngine.apply({ op: 'Remove', args: { id: obj.id } });
        } catch {
          // Object may already be gone if the engine reset; safe to ignore.
        }
      }
      this.handleClearSelection();
      this.dragApplied.clear();
      // The plate's objects are being replaced with new ids, so the previous
      // undo/redo history now points at objects that will no longer exist —
      // restoring one would delete the new plate's objects instead of
      // reverting an edit. This eviction is the one place identities actually
      // change, so void the history exactly here.
      this.sceneCommand.reset();
      this.activeModelSource = model;
    } else if (reopeningSamePlate) {
      // Returning to a plate the engine already holds. Keep its objects (and
      // the history that references them) intact; only this fresh component's
      // stale Three.js mirror and selection need clearing before it re-mirrors
      // the engine below.
      for (const id of this.wasmMeshes.keys()) {
        this.scene?.unregisterSelectable(String(id));
      }
      this.wasmMeshes.clear();
      this.handleClearSelection();
      this.activeModelSource = model;
    } else {
      // Mode switch only (e.g. model → gcode → model). The WASM scene engine
      // still holds the object with its current transforms intact. Unregister
      // the now-disposed Three.js selectables so raycasts do not hit stale
      // nodes, but do NOT issue Remove ops — that would wipe the transforms.
      for (const id of this.wasmMeshes.keys()) {
        this.scene?.unregisterSelectable(String(id));
      }
      this.wasmMeshes.clear();
      this.handleClearSelection();
    }

    if (mode === 'model') {
      if (!model) {
        this.status.set('idle');
        return;
      }
      // If the WASM engine already holds objects for this source (mode switch,
      // or returning to the same plate), re-render directly from engine state
      // so transforms are preserved without a second parse round-trip.
      const existingObjects = untracked(() => this.sceneEngine.objects());
      if ((!modelChanged || reopeningSamePlate) && existingObjects.length > 0) {
        void this.rebuildThreeJsMeshes();
      } else {
        this.startModelLoad(model);
      }
    } else {
      // G-code is rendered exclusively through the WASM GcodeHandle path,
      // which gives per-layer / per-role geometry. The old TS streaming
      // fallback (ChunkedLineGeometry / startGcodeLoad) is intentionally
      // not used: it produced a flat cyan mesh and was only ever a
      // temporary stand-in before the WASM parser existed.
      // Read gcodeHandle untracked: layer/role/progress changes flow through
      // their own dedicated effects, and we must not rebuild the whole layer
      // graph (and re-fit the camera) on every slider tick.
      if (untracked(() => this.gcodePreview.gcodeHandle())) {
        this.startGcodeFromHandle();
      } else {
        // Handle not yet available — either loading is in progress or no
        // source has been dispatched yet. Hold at loading/idle and let the
        // gcodeHandle effect below call startGcodeFromHandle once ready.
        this.status.set(untracked(() => this.gcodePreview.loading()) ? 'loading' : 'idle');
      }
    }
  }

  /**
   * Re-render Three.js display meshes from objects already held by the WASM
   * scene engine. Called when switching back to model view after a mode switch
   * (e.g. model → gcode → model) so that user-applied transforms are not lost.
   */
  private async rebuildThreeJsMeshes(): Promise<void> {
    await this.sceneEngine.ready();
    if (!this.scene) {
      return;
    }
    this.syncWasmMeshes();
    this.status.set('ready');
    this.loadComplete.emit({ mode: 'model', segments: 0 });
  }

  /**
   * Reconcile the Three.js display nodes with the scene engine's object list.
   *
   * The engine is the source of truth for *what* is on the plate, so the
   * viewer mirrors it rather than tracking adds and removes itself. That is
   * what lets an object added from anywhere — the add-object button, a
   * duplicate, an undo — appear without the viewer knowing who did it.
   *
   * Objects are diffed by id: new ones get a display mesh, vanished ones are
   * disposed. Untouched ids keep their existing geometry, so adding a second
   * model never re-uploads or re-parses the first.
   */
  private syncWasmMeshes(): void {
    const scene = this.scene;
    if (!scene) {
      return;
    }
    const objects = untracked(() => this.sceneEngine.objects());
    const live = new Set(objects.map((o) => o.id));

    for (const [id, mesh] of [...this.wasmMeshes]) {
      if (live.has(id)) {
        continue;
      }
      scene.unregisterSelectable(String(id));
      scene.contentRoot.remove(mesh);
      mesh.geometry.dispose();
      disposeMaterial(mesh.material);
      this.wasmMeshes.delete(id);
    }

    // Forget removed objects everywhere the selection is mirrored, so the
    // transform panel and gizmo cannot act on an id that no longer exists.
    const pruned = this.selectedWasmIds.filter((id) => live.has(id));
    if (pruned.length !== this.selectedWasmIds.length) {
      this.selectedWasmIds = pruned;
      scene.setSelectedIds(new Set(pruned.map(String)));
      this.viewerControl.selectedObjectIds.set(pruned);
    }

    for (const obj of objects) {
      if (this.wasmMeshes.has(obj.id)) {
        continue;
      }
      const mesh = this.buildDisplayMesh(obj.id, obj.name);
      scene.contentRoot.add(mesh);
      this.wasmMeshes.set(obj.id, mesh);
      scene.registerSelectable(String(obj.id), mesh);
    }

    scene.invalidate();
  }

  /**
   * Build the Three.js display node for one scene-engine object.
   *
   * The node is a thin mirror: geometry comes from the WASM render buffer and
   * the matrix is driven by the engine, so `matrixAutoUpdate` stays off.
   */
  private buildDisplayMesh(id: bigint, name: string): Mesh {
    const buf = this.sceneEngine.getRenderBuffer(id);
    const geometry = new BufferGeometry();
    geometry.setAttribute('position', new BufferAttribute(buf.positions, 3));
    geometry.setAttribute('normal', new BufferAttribute(buf.normals, 3));
    geometry.setIndex(new BufferAttribute(buf.indices, 1));
    geometry.computeBoundingBox();
    geometry.computeBoundingSphere();
    const material = new MeshPhongMaterial({
      color: this.currentModelColor(),
      flatShading: true,
      shininess: 16,
    });
    const mesh = new Mesh(geometry, material);
    mesh.name = name;
    mesh.matrixAutoUpdate = false;
    this.tmpMatrix.fromArray(this.sceneEngine.getMatrix(id));
    mesh.matrix.copy(this.tmpMatrix);
    mesh.matrixWorldNeedsUpdate = true;
    // Precompute coplanar face groups and store in userData so the
    // pull-to-floor highlight can light up whole flat regions rather than
    // individual triangles. Groups are computed once here in WASM (O(F) with
    // union-find) and read O(1) per hover frame afterwards.
    mesh.userData['faceGroups'] = this.sceneEngine.getFaceGroups(id);
    return mesh;
  }

  private startModelLoad(source: ModelSource): void {
    const scene = this.scene;
    if (!scene) {
      return;
    }
    const token = ++this.loadToken;
    this.status.set('loading');

    // New WASM-scene-engine path: fetch raw bytes, parse them inside
    // Rust, then build a BufferGeometry from the WASM-emitted render
    // buffer. The scene-engine owns the mesh data and the transform; the
    // Three.js node is a thin display mirror with `matrixAutoUpdate = false`.
    void this.loadModelViaSceneEngine(source, token).catch((error: unknown) => {
      if (token !== this.loadToken) {
        return;
      }
      this.errorMessage.set(messageOf(error));
      this.status.set('error');
      this.loadError.emit({ mode: 'model', error });
    });
  }

  private async loadModelViaSceneEngine(source: ModelSource, token: number): Promise<void> {
    await this.sceneEngine.ready();
    if (token !== this.loadToken || !this.scene) {
      return;
    }
    const { bytes, format, name } = await readModelBytes(source);
    if (token !== this.loadToken || !this.scene) {
      return;
    }
    const sourceId = untracked(() => this.modelSourceId()) ?? undefined;
    // Time each phase of the WASM round-trip independently so the overlay
    // can break down where wall time is spent (parse vs. render-buffer
    // copy). `performance.now()` returns a high-resolution monotonic clock.
    const tParseStart = performance.now();
    const ids = this.sceneEngine.addMesh(name, format, bytes, sourceId);
    const tParseEnd = performance.now();
    // Auto-orient and drop to bed on first load. Applied directly through
    // the engine (not sceneCommand) so the oriented position is the baseline
    // state and Ctrl+Z does not revert back to the un-oriented pose.
    // A 3MF can yield several parts, so orient each one.
    for (const id of ids) {
      this.sceneEngine.apply({ op: 'AutoOrient', args: { id } });
      this.sceneEngine.apply({ op: 'DropToFloor', args: { id } });
    }
    // Build the display node from the engine's object list rather than by
    // hand, so this path and every other add share one code path.
    this.syncWasmMeshes();
    const tRenderBufEnd = performance.now();
    this.wasmParseMs.set(tParseEnd - tParseStart);
    this.wasmRenderBufMs.set(tRenderBufEnd - tParseEnd);
    this.wasmRoundtripMs.set(tRenderBufEnd - tParseStart);
    this.status.set('ready');
    this.loadComplete.emit({ mode: 'model', segments: 0 });
  }

  /**
   * Render gcode using the parsed `GcodeHandle` from `GcodePreviewService`.
   * Delegates geometry construction to `GcodeOrchestrator`; Three.js only
   * manages layer/segment visibility after this point.
   */
  private startGcodeFromHandle(): void {
    const scene = this.scene;
    const gcode = this.gcode;
    // All gcode-preview reads here must be untracked. This method runs from
    // the `applySource` effect; if any of the layer/role/progress signals
    // were tracked here, every slider tick would re-enter this path and
    // rebuild every layer group + re-fit the camera. The dedicated effects
    // (showRange / applyProgress / applyHiddenRoles) are the sole reactive
    // consumers of those signals.
    const handle = untracked(() => this.gcodePreview.gcodeHandle());
    if (!scene || !gcode || !handle) {
      return;
    }

    this.cancelInFlightLoad();
    const colors = untracked(() => this.gcodePreview.roleColors());
    const { totalSegments } = gcode.buildFromHandle(handle, colors);

    const min = untracked(() => this.gcodePreview.layerMin());
    const max = untracked(() => this.gcodePreview.layerMax());
    const progress = untracked(() => this.gcodePreview.segmentProgress());
    const hidden = untracked(() => this.gcodePreview.hiddenRoles());
    const mode = untracked(() => this.gcodePreview.effectiveViewMode());
    const range = untracked(() => this.gcodePreview.activeRange());
    const fan = untracked(() => this.gcodePreview.selectedFan());
    const band = untracked(() => this.gcodePreview.hoverBand());
    gcode.applyView(colors, scalarChannelFor(mode), range, fan, band);
    gcode.showRange(min, max);
    gcode.applyProgress(max, progress);
    gcode.applyHiddenRoles(hidden);
    // A new plate gets a fresh verdict — the previous one may have been much
    // heavier (or lighter) than this one.
    this.fullDetailAffordable = true;
    this.probingDetail = false;
    this.refreshGcodeDetail();
    scene.invalidate();
    this.status.set('ready');
    this.loadComplete.emit({ mode: 'gcode', segments: totalSegments });
  }

  private cancelInFlightLoad(): void {
    this.loadToken++;
    if (this.currentAbort) {
      this.currentAbort.abort();
      this.currentAbort = null;
    }
  }

  private currentModelColor(): number {
    return resolveModelColor(
      this.appTheme.isDarkMode(),
      this.viewerControl.useFilamentColor(),
      this.activeSelection.filament()?.color,
    );
  }

  private async captureSliceThumbnail(
    request: SliceThumbnailRequest,
  ): Promise<SliceThumbnailCapture | null> {
    const scene = this.scene;
    if (!scene) {
      return null;
    }

    const targetSize = clampThumbnailSize(request.sizePx);
    const pose = THUMBNAIL_VIEW_POSES[request.view] ?? THUMBNAIL_VIEW_POSES.isometric;
    const isTransparent = request.theme === 'transparent';
    const thumbIsDark = request.theme === 'dark';
    const liveIsDark = this.appTheme.isDarkMode();

    // The thumbnail has its own colour mode (generic / filament / custom),
    // independent of the viewer's live filament-colour toggle.
    const filamentColor = this.activeSelection.filament()?.color;
    const thumbColor = resolveThumbnailColor(
      thumbIsDark,
      request.colorMode,
      filamentColor,
      request.customColor,
    );

    // Build fresh model meshes from the WASM scene engine so the thumbnail
    // always depicts the model — even when the viewer is currently showing the
    // G-code preview (whose toolpaths would otherwise be captured and skew the
    // framing). These are throwaway meshes, never added to the live scene.
    const subjects = this.buildThumbnailSubjects(thumbColor);

    let dataUrl: string | null = null;
    try {
      dataUrl = scene.captureThumbnail({
        sizePx: targetSize,
        direction: pose.dir.clone(),
        up: pose.up.clone(),
        isDark: thumbIsDark,
        liveIsDark,
        background: isTransparent ? null : thumbIsDark ? THUMBNAIL_BG_DARK : THUMBNAIL_BG_LIGHT,
        subjects: subjects.length > 0 ? subjects : undefined,
      });
    } finally {
      for (const mesh of subjects) {
        mesh.geometry.dispose();
        (mesh.material as MeshPhongMaterial).dispose();
      }
    }

    if (!dataUrl) {
      return null;
    }
    const comma = dataUrl.indexOf(',');
    if (comma < 0) {
      return null;
    }
    const pngBase64 = dataUrl.slice(comma + 1);

    // Fling the shutter-flash + polaroid FX only when the user is looking at
    // the model *and* this capture is actually a different image from the last
    // one. Re-slicing an unchanged scene — tweaking a non-visual setting, or a
    // cache-hit re-slice — reproduces a byte-identical thumbnail from the fixed
    // capture viewpoint, so re-playing the same polaroid would just be noise.
    // The reference is refreshed on every capture (in either mode) so it always
    // tracks the thumbnail currently embedded in the print.
    const changed = pngBase64 !== this.lastThumbnailImage;
    this.lastThumbnailImage = pngBase64;
    if (changed && this.mode() === 'model') {
      this.playThumbnailCaptureFx(dataUrl);
    }
    return {
      pngBase64,
      sizePx: targetSize,
    };
  }

  /**
   * Build detached Three.js meshes for the current model objects straight from
   * the WASM scene engine's render buffers, painted in the thumbnail colour.
   * These are used solely for an off-screen thumbnail render and disposed by
   * the caller — they are never added to the live scene or the selectable set,
   * so this works identically whether the viewer is in model or G-code mode.
   */
  private buildThumbnailSubjects(color: number): Mesh[] {
    const subjects: Mesh[] = [];
    const objects = untracked(() => this.sceneEngine.objects());
    for (const obj of objects) {
      let buf: ReturnType<SceneEngine['getRenderBuffer']>;
      try {
        buf = this.sceneEngine.getRenderBuffer(obj.id);
      } catch {
        continue;
      }
      const geometry = new BufferGeometry();
      geometry.setAttribute('position', new BufferAttribute(buf.positions, 3));
      geometry.setAttribute('normal', new BufferAttribute(buf.normals, 3));
      geometry.setIndex(new BufferAttribute(buf.indices, 1));
      geometry.computeBoundingBox();
      geometry.computeBoundingSphere();
      const material = new MeshPhongMaterial({ color, flatShading: true, shininess: 16 });
      const mesh = new Mesh(geometry, material);
      mesh.matrixAutoUpdate = false;
      this.tmpMatrix.fromArray(this.sceneEngine.getMatrix(obj.id));
      mesh.matrix.copy(this.tmpMatrix);
      mesh.matrixWorldNeedsUpdate = true;
      subjects.push(mesh);
    }
    return subjects;
  }

  private playThumbnailCaptureFx(dataUrl: string): void {
    this.clearThumbnailFxTimers();
    this.thumbnailShutterActive.set(false);
    this.thumbnailPolaroidAnimating.set(false);
    this.thumbnailPolaroidImage.set(dataUrl);

    requestAnimationFrame(() => {
      this.thumbnailShutterActive.set(true);
      this.thumbnailPolaroidAnimating.set(true);
    });

    this.shutterTimer = setTimeout(() => {
      this.thumbnailShutterActive.set(false);
    }, THUMBNAIL_SHUTTER_MS);
    this.polaroidTimer = setTimeout(() => {
      this.thumbnailPolaroidAnimating.set(false);
      this.thumbnailPolaroidImage.set(null);
    }, THUMBNAIL_POLAROID_MS);
  }

  private clearThumbnailFxTimers(): void {
    if (this.shutterTimer !== null) {
      clearTimeout(this.shutterTimer);
      this.shutterTimer = null;
    }
    if (this.polaroidTimer !== null) {
      clearTimeout(this.polaroidTimer);
      this.polaroidTimer = null;
    }
  }
}

function messageOf(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === 'string') {
    return error;
  }
  return '';
}

/** Release a mesh's material(s), which Three.js does not dispose with the node. */
function disposeMaterial(material: Mesh['material']): void {
  if (Array.isArray(material)) {
    for (const m of material) {
      m.dispose();
    }
    return;
  }
  material.dispose();
}

/** Same ids in the same order? Used to break selection mirror feedback. */
function sameIds(a: readonly bigint[], b: readonly bigint[]): boolean {
  return a.length === b.length && a.every((id, i) => id === b[i]);
}
/**
 * Coerce a {@link ModelSource} into the `{ bytes, format, name }` triple
 * expected by `SceneEngineService.addMesh`. The format is detected from the
 * source's filename / URL extension; raw `ArrayBuffer`/`Blob` inputs without
 * a name fall back to STL (the most common payload).
 */
async function readModelBytes(
  source: ModelSource,
): Promise<{ bytes: Uint8Array; format: 'stl' | 'obj' | '3mf'; name: string }> {
  let buffer: ArrayBuffer;
  let name = 'model';
  if (typeof source === 'string') {
    name = source.split(/[\\/]/).pop() ?? 'model';
    const res = await fetch(source);
    buffer = await res.arrayBuffer();
  } else if (source instanceof URL) {
    name = source.pathname.split('/').pop() ?? 'model';
    const res = await fetch(source);
    buffer = await res.arrayBuffer();
  } else if (source instanceof File) {
    name = source.name;
    buffer = await source.arrayBuffer();
  } else if (source instanceof Blob) {
    buffer = await source.arrayBuffer();
  } else {
    buffer = source;
  }
  const ext = name.split('.').pop()?.toLowerCase();
  const format: 'stl' | 'obj' | '3mf' =
    ext === 'obj' || ext === '3mf' || ext === 'stl' ? ext : 'stl';
  return { bytes: new Uint8Array(buffer), format, name };
}

/**
 * Parse the string id stored on the legacy scene's selectable registry back
 * into the WASM `bigint` id used by the scene engine. Returns `null` if the
 * string isn't a valid integer (defensive — should never happen since we
 * stamp the id ourselves at registration time).
 */
function parseWasmId(stringId: string): bigint | null {
  try {
    return BigInt(stringId);
  } catch {
    return null;
  }
}

function resolveModelColor(
  isDark: boolean,
  useFilamentColor: boolean,
  filamentColor: string | null | undefined,
): number {
  if (!useFilamentColor) {
    return modelColor(isDark);
  }
  const parsed = parseHexColor(filamentColor);
  return parsed ?? modelColor(isDark);
}

/**
 * Resolve the model colour for the thumbnail's own colour mode. `generic` uses
 * the theme-tuned neutral grey; `filament` uses the active filament colour;
 * `custom` uses the chosen hex. Any unparseable value falls back to grey.
 */
function resolveThumbnailColor(
  isDark: boolean,
  mode: ThumbnailColorMode,
  filamentColor: string | null | undefined,
  customColor: string,
): number {
  switch (mode) {
    case 'filament':
      return parseHexColor(filamentColor) ?? modelColor(isDark);
    case 'custom':
      return parseHexColor(customColor) ?? modelColor(isDark);
    case 'generic':
    default:
      return modelColor(isDark);
  }
}

function parseHexColor(raw: string | null | undefined): number | null {
  if (!raw) {
    return null;
  }
  const trimmed = raw.trim();
  const match = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(trimmed);
  if (!match) {
    return null;
  }
  const hex = match[1];
  const normalized =
    hex.length === 3 ? `${hex[0]}${hex[0]}${hex[1]}${hex[1]}${hex[2]}${hex[2]}` : hex.toLowerCase();
  return Number.parseInt(normalized, 16);
}

function clampThumbnailSize(sizePx: number): number {
  if (!Number.isFinite(sizePx)) {
    return 320;
  }
  return Math.max(64, Math.min(1024, Math.round(sizePx)));
}
