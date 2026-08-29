import { inject, Injectable, signal } from '@angular/core';
import { Vector3 } from 'three';
import type { ViewerMode } from '../components/viewer';
import { BrowserStorage } from './browser-storage';

export type ViewerView = 'perspective' | 'ortho';
/**
 * Object-manipulation mode. Drives the on-canvas gizmo for the current
 * selection. `'none'` is the default — no gizmo is shown, clicks select.
 * `'pullToFloor'` is a transient face-pick mode that auto-exits to
 * `'none'` after a single face has been picked.
 */
export type ObjectMode = 'none' | 'translate' | 'rotate' | 'scale' | 'pullToFloor';

/**
 * Which camera action a bare two-finger trackpad swipe performs on macOS.
 * `'orbit'` (default) matches Shapr3D; `'pan'` lets the user pan without
 * holding ⌥ (orbit then moves to ⌥ + swipe). Windows/Linux are unaffected
 * — their trackpad wheel path only zooms.
 */
export type TwoFingerGesture = 'orbit' | 'pan';

/**
 * Anti-aliasing preference for the 3D view. `'auto'` disables MSAA on
 * high-DPI (≥2×) displays — where the extra samples buy little and cost
 * performance — and enables it everywhere else. `'on'`/`'off'` force it.
 * Applied at renderer construction, so a change rebuilds the scene.
 */
export type Antialiasing = 'auto' | 'on' | 'off';

/**
 * Render-resolution quality. Caps the device-pixel-ratio the renderer draws
 * at: `'performance'` = 1×, `'balanced'` = up to 2×, `'quality'` = up to 3×.
 * Higher is sharper but more expensive. Applied live via `setPixelRatio`.
 */
export type RenderQuality = 'performance' | 'balanced' | 'quality';

/**
 * How much geometry the sliced-G-code preview draws.
 *
 * Deliberately separate from {@link RenderQuality}, which is render
 * *resolution* (a pixel-ratio cap). This is the *geometry* axis: how many
 * triangles each extrusion bead is worth.
 *
 * - `'auto'` — full detail whenever it is affordable, which thanks to the
 *   on-demand render loop includes every still view; only active interaction on
 *   a heavy plate falls back to the cheap bead. Self-tunes by measuring real
 *   frame cost, so it adapts to the machine instead of guessing from a GPU name.
 * - `'performance'` — always the cheap bead.
 * - `'quality'` — always the full bead, even while orbiting a huge plate.
 */
export type PreviewDetail = 'auto' | 'performance' | 'quality';

export interface SliceThumbnailCapture {
  pngBase64: string;
  sizePx: number;
}

/** Fixed camera angle for the embedded thumbnail render. */
export type ThumbnailView = 'isometric' | 'front' | 'rear' | 'left' | 'right' | 'top';

/** Fixed colour scheme for the embedded thumbnail render. */
export type ThumbnailTheme = 'light' | 'dark' | 'transparent';

/** How the model is coloured in the thumbnail. */
export type ThumbnailColorMode = 'generic' | 'filament' | 'custom';

/** A request to render the outbound slice thumbnail from a fixed viewpoint. */
export interface SliceThumbnailRequest {
  sizePx: number;
  view: ThumbnailView;
  theme: ThumbnailTheme;
  colorMode: ThumbnailColorMode;
  /** `#rrggbb` used when `colorMode === 'custom'`. */
  customColor: string;
}

/** Default perspective field-of-view in degrees. */
export const DEFAULT_FIELD_OF_VIEW = 45;
/** Allowed field-of-view range (degrees) for the settings slider. */
export const MIN_FIELD_OF_VIEW = 20;
export const MAX_FIELD_OF_VIEW = 80;

/** Map a {@link RenderQuality} to the maximum device-pixel-ratio cap. */
export function pixelRatioCapFor(quality: RenderQuality): number {
  switch (quality) {
    case 'performance':
      return 1;
    case 'quality':
      return 3;
    case 'balanced':
    default:
      return 2;
  }
}

/**
 * Resolve an {@link Antialiasing} preference to the concrete MSAA flag a
 * WebGLRenderer is built with. `'auto'` disables it on high-DPI (≥2×) displays
 * where the extra samples buy little. Shared by the main viewer and any
 * preview scenes so they stay in lock-step.
 */
export function resolveAntialias(mode: Antialiasing): boolean {
  if (mode === 'on') {
    return true;
  }
  if (mode === 'off') {
    return false;
  }
  return !(typeof window !== 'undefined' && window.devicePixelRatio >= 2);
}

const TWO_FINGER_GESTURE_KEY = 'nexus.viewer.trackpadTwoFingerGesture';
const STATS_VISIBLE_KEY = 'nexus.viewer.statsVisible';
const FIELD_OF_VIEW_KEY = 'nexus.viewer.fieldOfView';
const ANTIALIASING_KEY = 'nexus.viewer.antialiasing';
const RENDER_QUALITY_KEY = 'nexus.viewer.renderQuality';
const PREVIEW_DETAIL_KEY = 'nexus.viewer.previewDetail';
const USE_FILAMENT_COLOR_KEY = 'nexus.viewer.useFilamentColor';
const PALM_REJECTION_KEY = 'nexus.viewer.palmRejection';

/**
 * Shared state between the 3D-view toolbar and the viewer component.
 *
 * The toolbar lives in the layout shell and the viewer in the routed page,
 * so the two are wired together through this lightweight signal-based store
 * rather than via component I/O.
 */
@Injectable({ providedIn: 'root' })
export class ViewerControl {
  private readonly storage = inject(BrowserStorage);

  /**
   * macOS trackpad two-finger swipe action. Persisted to localStorage so the
   * choice survives reloads. The viewer pushes it into the Three.js
   * SceneControls; it is changed from the Keyboard Shortcuts dialog. Offers a
   * keyboard-free pan alternative: set it to `'pan'` and two-finger swipe
   * pans (orbit then requires ⌥).
   */
  readonly trackpadTwoFingerGesture = signal<TwoFingerGesture>(this.readTwoFingerGesture());

  /**
   * Whether scene telemetry chips (FPS/WASM/op timings) are visible.
   *
   * Default is `true` for the beta app so diagnostics stay easy to access.
   * Persisted to localStorage and can later be flipped to default `false`
   * for production-focused builds.
   */
  readonly statsVisible = signal(this.readStatsVisible());

  /** Currently selected camera view preset. */
  readonly view = signal<ViewerView>('perspective');

  /** Whether the viewport shows the raw mesh ('model') or sliced G-code ('gcode'). */
  readonly viewMode = signal<ViewerMode>('model');

  /**
   * Perspective field-of-view in degrees. Persisted. The viewer pushes it
   * into the SceneCamera live; the ortho preset ignores it (it forces a ~1°
   * FOV to fake an orthographic projection).
   */
  readonly fieldOfView = signal<number>(this.readFieldOfView());

  /**
   * Anti-aliasing preference. Persisted. Applied at renderer construction,
   * so the viewer rebuilds its scene when this changes.
   */
  readonly antialiasing = signal<Antialiasing>(this.readAntialiasing());

  /**
   * Render-resolution quality (device-pixel-ratio cap). Persisted and applied
   * live via the renderer's pixel ratio.
   */
  readonly renderQuality = signal<RenderQuality>(this.readRenderQuality());

  /**
   * G-code preview geometry detail. Persisted and applied live — no reload or
   * re-slice needed, since both bead LODs are built up front.
   */
  readonly previewDetail = signal<PreviewDetail>(this.readPreviewDetail());

  /**
   * Whether model meshes use the active filament profile color instead of the
   * neutral theme-based graphite tone.
   *
   * Default is `false` to preserve the existing scene appearance.
   */
  readonly useFilamentColor = signal(this.readUseFilamentColor());

  /**
   * Whether pen-priority palm rejection ("wrist detection") is active in the
   * 3D view. When on (the default), touch contacts from the hand resting on
   * the glass are ignored while an Apple Pencil / stylus is in use, so the palm
   * never orbits or pinches the camera. Pure-touch gestures are unaffected.
   * Persisted so the choice survives reloads.
   */
  readonly palmRejection = signal(this.readPalmRejection());

  /**
   * Currently selected object-manipulation mode. Drives the gizmo shown
   * over the current selection. Independent of camera orbit/pan — the
   * user picks a camera mode and an object mode separately.
   */
  readonly objectMode = signal<ObjectMode>('translate');

  /**
   * WASM scene-engine ids of the currently selected objects, published by
   * the viewer as the user clicks meshes. Shared here (rather than kept
   * private to the viewer) so the toolbar's transform sub-settings panel can
   * read which object is selected and drive absolute-value edits against it.
   */
  readonly selectedObjectIds = signal<readonly bigint[]>([]);

  /**
   * Monotonically increasing counter that is bumped every time the user
   * asks the viewer to reset its camera. The viewer reacts to changes of
   * this signal — the value itself is irrelevant.
   */
  readonly resetTick = signal(0);

  /**
   * Live camera orientation, updated by the viewer every frame. Read by the
   * viewport-cube gizmo (which mirrors the main camera in its own scene)
   * without going through Angular's change-detection pipeline.
   */
  readonly cameraState = {
    /** Unit vector from the controls target toward the camera. */
    direction: new Vector3(1, -1, 0.8).normalize(),
    /** Camera up vector. */
    up: new Vector3(0, 0, 1),
    /**
     * Live perspective field-of-view (degrees) of the main camera. The
     * viewport-cube mirrors this so its own projection matches — small FOV
     * (~1°) reads as orthographic, ~45° as perspective.
     */
    fov: 45,
  };

  /**
   * When `true`, every completed object-manipulation gesture automatically
   * drops the affected objects to the floor (applies `DropToFloor`) so
   * objects never float above the bed after being moved or rotated.
   */
  readonly gravityEnabled = signal(false);

  /**
   * Pending request for the viewer to animate to a specific look direction
   * (e.g. when the user clicks a face of the viewport-cube). Cleared after
   * the viewer consumes it; the `tick` field disambiguates repeated requests
   * for the same direction. `autoOrtho` asks the viewer to also snap the
   * projection to orthographic (viewport-cube behaviour) until the next free
   * pan/zoom.
   */
  readonly lookRequest = signal<{
    direction: Vector3;
    up: Vector3;
    tick: number;
    autoOrtho: boolean;
  } | null>(null);
  private lookTick = 0;

  /**
   * Pending request to roll the camera about its view axis by `radians`
   * (animated). Emitted by the viewport-cube's roll buttons; consumed by the
   * viewer. The `tick` disambiguates repeated rolls in the same direction.
   */
  readonly rollRequest = signal<{ radians: number; tick: number } | null>(null);
  private rollTick = 0;

  /**
   * Direct callback for high-frequency incremental orbit deltas (radians).
   * Set by the viewer; invoked by the viewport-cube gizmo while the user
   * drags it. Bypasses signal/effect overhead.
   */
  orbitSink: ((azimuth: number, polar: number) => void) | null = null;

  /**
   * Optional callback exposed by the active 3D viewer to render a square PNG
   * thumbnail from a fixed camera angle and theme (see
   * {@link SliceThumbnailRequest}) — deliberately not the live viewport.
   */
  sliceThumbnailCaptureSink:
    ((request: SliceThumbnailRequest) => Promise<SliceThumbnailCapture | null>) | null = null;

  async captureSliceThumbnail(
    request: SliceThumbnailRequest,
  ): Promise<SliceThumbnailCapture | null> {
    const sink = this.sliceThumbnailCaptureSink;
    if (!sink) {
      return null;
    }
    return sink(request);
  }

  /** Request the viewer to fully reset its camera framing. */
  reset(): void {
    this.view.set('perspective');
    this.resetTick.update((v) => v + 1);
  }

  /** Update the two-finger swipe preference and persist it to localStorage. */
  setTrackpadTwoFingerGesture(gesture: TwoFingerGesture): void {
    this.trackpadTwoFingerGesture.set(gesture);
    this.storage.write(TWO_FINGER_GESTURE_KEY, gesture);
  }

  /** Update telemetry visibility and persist the preference. */
  setStatsVisible(value: boolean): void {
    this.statsVisible.set(value);
    this.storage.write(STATS_VISIBLE_KEY, String(value));
  }

  /** Toggle telemetry visibility and persist the preference. */
  toggleStatsVisible(): void {
    this.setStatsVisible(!this.statsVisible());
  }

  /** Update the perspective field-of-view (degrees), clamped, and persist it. */
  setFieldOfView(fov: number): void {
    const clamped = Math.round(Math.max(MIN_FIELD_OF_VIEW, Math.min(MAX_FIELD_OF_VIEW, fov)));
    this.fieldOfView.set(clamped);
    this.storage.write(FIELD_OF_VIEW_KEY, String(clamped));
  }

  /** Update the anti-aliasing preference and persist it. */
  setAntialiasing(mode: Antialiasing): void {
    this.antialiasing.set(mode);
    this.storage.write(ANTIALIASING_KEY, mode);
  }

  /** Update the render-resolution quality and persist it. */
  setRenderQuality(quality: RenderQuality): void {
    this.renderQuality.set(quality);
    this.storage.write(RENDER_QUALITY_KEY, quality);
  }

  /** Update G-code preview geometry detail and persist it. */
  setPreviewDetail(detail: PreviewDetail): void {
    this.previewDetail.set(detail);
    this.storage.write(PREVIEW_DETAIL_KEY, detail);
  }

  /** Update model-color source preference and persist it. */
  setUseFilamentColor(value: boolean): void {
    this.useFilamentColor.set(value);
    this.storage.write(USE_FILAMENT_COLOR_KEY, String(value));
  }

  /** Update the palm-rejection preference and persist it. */
  setPalmRejection(value: boolean): void {
    this.palmRejection.set(value);
    this.storage.write(PALM_REJECTION_KEY, String(value));
  }

  private readTwoFingerGesture(): TwoFingerGesture {
    return this.storage.get(TWO_FINGER_GESTURE_KEY)() === 'pan' ? 'pan' : 'orbit';
  }

  private readStatsVisible(): boolean {
    const raw = this.storage.get(STATS_VISIBLE_KEY)();
    if (raw === 'false') {
      return false;
    }
    if (raw === 'true') {
      return true;
    }
    return true;
  }

  private readFieldOfView(): number {
    const raw = Number(this.storage.get(FIELD_OF_VIEW_KEY)());
    if (!Number.isFinite(raw)) {
      return DEFAULT_FIELD_OF_VIEW;
    }
    return Math.round(Math.max(MIN_FIELD_OF_VIEW, Math.min(MAX_FIELD_OF_VIEW, raw)));
  }

  private readAntialiasing(): Antialiasing {
    const raw = this.storage.get(ANTIALIASING_KEY)();
    return raw === 'on' || raw === 'off' ? raw : 'auto';
  }

  private readRenderQuality(): RenderQuality {
    const raw = this.storage.get(RENDER_QUALITY_KEY)();
    return raw === 'performance' || raw === 'quality' ? raw : 'balanced';
  }

  private readPreviewDetail(): PreviewDetail {
    const raw = this.storage.get(PREVIEW_DETAIL_KEY)();
    return raw === 'performance' || raw === 'quality' ? raw : 'auto';
  }

  private readUseFilamentColor(): boolean {
    return this.storage.get(USE_FILAMENT_COLOR_KEY)() === 'true';
  }

  private readPalmRejection(): boolean {
    // Default on — palm rejection only changes behaviour once a pen appears,
    // so it is safe to enable everywhere.
    return this.storage.get(PALM_REJECTION_KEY)() !== 'false';
  }

  /**
   * Ask the viewer to animate to a specific camera direction (unit vector
   * from the controls target toward the camera) with the given up vector.
   * The current target and distance are preserved.
   *
   * @param autoOrtho  When `true` (viewport-cube snaps), also flatten the
   *   projection to orthographic until the next free pan/zoom in the viewport.
   */
  lookFrom(direction: Vector3, up: Vector3, autoOrtho = false): void {
    this.lookTick += 1;
    this.lookRequest.set({
      direction: direction.clone().normalize(),
      up: up.clone().normalize(),
      tick: this.lookTick,
      autoOrtho,
    });
  }

  /** Ask the viewer to roll the camera about its view axis by `radians`. */
  roll(radians: number): void {
    this.rollTick += 1;
    this.rollRequest.set({ radians, tick: this.rollTick });
  }
}
