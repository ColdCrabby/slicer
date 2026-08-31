import {
  MOUSE,
  type PerspectiveCamera,
  Plane,
  Quaternion,
  Raycaster,
  Spherical,
  TOUCH,
  Vector2,
  Vector3,
  type WebGLRenderer,
} from 'three';
import type { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { isSyntheticPointerEvent, markSyntheticPointerEvent } from './synthetic-pointer';
import { type TwoFingerSample, TwoFingerGestureTracker } from './two-finger-gesture';

const TOUCH_DISABLED = -1 as unknown as TOUCH;
/**
 * Separation change (px, accumulated since the last applied dolly) before a
 * pinch starts zooming. Accumulated rather than measured per frame: at 120 Hz a
 * deliberate slow pinch moves well under a pixel per event, so a per-frame test
 * discarded the whole gesture and the camera never zoomed at all.
 */
const TWO_FINGER_DOLLY_DEAD_ZONE_PX = 1.5;
/**
 * Twist (rad, accumulated since the last applied roll) before an *engaged* roll
 * moves the camera. Only consulted after {@link ROLL_ENGAGE_ANGLE_RAD} has
 * already qualified the gesture as a twist — this is fine-grain smoothing, not
 * the gate.
 */
const TWO_FINGER_ROLL_DEAD_ZONE_RAD = 0.01;
/**
 * Net twist (rad ≈ 6°) that must build up from the start of a two-finger
 * gesture before roll engages at all.
 *
 * The old code had no such gate: it rolled whenever a single frame's angle
 * delta cleared {@link TWO_FINGER_ROLL_DEAD_ZONE_RAD}. Because that delta is
 * `perpendicular_jitter / separation`, the test gets *easier* the closer the
 * fingers are — 2 px of noise at 40 px apart is already 2.9°, and pinching in
 * drives the separation down. So an ordinary zoom sprayed roll every frame:
 * the "zooming makes it rotate" spin. Requiring a real, sustained twist first
 * puts the gate on the user's intent instead of on their tremor.
 *
 * Kept modest because the *dominance* test below is what actually separates a
 * twist from a pinch; this only has to outrun tremor.
 */
const ROLL_ENGAGE_ANGLE_RAD = 0.105;
/**
 * Minimum finger separation (px) for roll to be considered. Angular resolution
 * collapses as the fingers meet, so below this a twist reading is noise by
 * construction — no threshold on the angle itself can rescue it. Low enough to
 * admit a normal pinch-sized grip, which is the span most rolls are made with.
 */
const ROLL_MIN_SEPARATION_PX = 45;
/**
 * How far rotation must outweigh scaling for a gesture to count as a twist,
 * comparing radians of turn against the fractional change in separation.
 *
 * Both are per-unit-radius fingertip displacement (`θ·r` against `(s−1)·r`), so
 * the test is **scale-invariant** — it asks the same of a narrow grip as a wide
 * one. Comparing an *arc length* against an absolute pixel change instead, as
 * this originally did, is biased by the radius: it demanded 6° of twist with
 * the fingers 300 px apart but **30°** at 60 px, so a normal grip could not
 * roll at all.
 */
const ROLL_DOMINANCE_RATIO = 1.0;
/**
 * Fractional change in separation (0.2 = 20%) that permanently disqualifies
 * roll for the rest of the gesture. Once the user has clearly pinched, later
 * rotational drift — the wrist unavoidably turning as the fingers close — must
 * never be honoured. This latch is what makes "a zoom never becomes a spin" a
 * guarantee rather than a threshold that a jittery frame can beat.
 *
 * A *ratio*, not a pixel count, so a small grip and a wide one have to pinch
 * equally hard rather than equally far. Measured from the gesture's origin,
 * **not** summed per-event travel: the latter integrates contact jitter, which
 * at 120 Hz crosses any such figure from noise alone in well under a second and
 * locks roll out of every gesture. See the module doc in
 * `two-finger-gesture.ts`.
 */
const ROLL_LOCKOUT_PINCH_RATIO = 0.2;
/**
 * Largest roll applied from a single event (rad ≈ 17°). A jump beyond this is
 * not a wrist — it is a contact patch morphing, a palm being adopted into the
 * pair, or coalesced events after a stall. Clamping keeps such an artefact from
 * whipping the camera around.
 */
const ROLL_MAX_STEP_RAD = 0.3;
/**
 * Largest per-event dolly ratio. Guards the same class of discontinuity as
 * {@link ROLL_MAX_STEP_RAD}: a pair re-anchoring onto a different pair of
 * contacts can halve or double the separation in one event.
 */
const DOLLY_MAX_STEP_FACTOR = 1.6;
/**
 * Largest centroid pan applied from a single event (px). A real fingertip
 * cannot cross this much between events; a re-anchor or a palm can.
 */
const PAN_MAX_STEP_PX = 160;
/**
 * How long a tracked touch survives without any event before the two-finger
 * controller reclaims it.
 *
 * Without this a dropped `pointerup` — a finger lifted over the toolbar, a
 * pointer stolen by the OS, the app backgrounded mid-pinch — strands a phantom
 * contact. The gesture never sees `touches.size === 0`, so it never ends, and
 * `controls.enabled` stays `false`: the camera is dead until the page reloads.
 *
 * Pruning deliberately runs **only when a new contact lands**, never on move. A
 * stationary finger emits no events, so a user who pauses mid-pinch to think
 * would otherwise have their live gesture torn down underneath them. Waiting
 * for the next `pointerdown` costs nothing — the user is touching the screen
 * again, which is exactly when a stuck gesture needs clearing — and the window
 * and visibility listeners already cover every case where the lift is merely
 * delivered somewhere else.
 */
const TOUCH_STALE_MS = 3000;
/**
 * Right-drag travel (px) before it counts as a genuine pan for releasing the
 * viewport-cube snap's frozen detent (not the ortho projection — pan is
 * sticky). Avoids releasing on a bare right-click that never moves.
 */
const RIGHT_PAN_RELEASE_THRESHOLD_PX = 3;
/**
 * Straight-line pointer travel (px) that breaks a viewport-cube snap free.
 *
 * Sticky, Shapr3D-style: while a snap is held the camera does **not move at
 * all** — a rotate gesture below this distance is absorbed completely, so the
 * dimension-true view survives accidental jitter, a small screen touch or a
 * stray nudge. Cross it and the snap "pops": the camera starts orbiting from
 * that point and the projection animates back (see
 * {@link SceneCamera.applySnapHold}).
 *
 * Measured from where the drag *started*, so wiggling back and forth never
 * breaks out — you have to genuinely pull away from the detent.
 */
const SNAP_BREAKOUT_TRAVEL_PX = 70;
/**
 * Idle gap (ms) that ends one trackpad two-finger-swipe rotate burst. That mac
 * wheel-orbit path has no pointer up/down to bracket the gesture, so a pause
 * longer than this starts a fresh breakout budget.
 */
const SNAP_BREAKOUT_IDLE_MS = 150;

/**
 * Safari/WebKit-proprietary gesture event (not in the standard DOM lib types).
 * Fired for trackpad pinch/rotate in WKWebView — i.e. the Tauri desktop app on
 * macOS. `scale` is cumulative relative to `gesturestart` (1.0 at the start).
 */
interface WebKitGestureEvent extends Event {
  readonly scale: number;
  readonly rotation: number;
  readonly clientX: number;
  readonly clientY: number;
}

const AUTOSCROLL_DEAD_ZONE_PX = 6;
const AUTOSCROLL_SPEED_PER_PX = 0.012;
const AUTOSCROLL_ACCEL_REF_PX = 100;
const AUTOSCROLL_ACCEL_EXPONENT = 4;
const AUTOSCROLL_MAX_FACTOR_PER_FRAME = 4;

// --- Mac trackpad tuning (Shapr3D-style two-finger gestures) --------------
// Two-finger swipe orbits at ~0.34° per pixel (200 px = ~68°).
// Option + swipe pans 1:1 with pixel deltas (grab feel).
// Pinch (ctrlKey wheel event synthesised by macOS) zooms toward the cursor.
//
// Pinch zoom uses a direct exponential model:
//     factor = exp(clamp(deltaY, ±MAX) * RATE)
// The per-event clamp is essential because Chrome/Safari pinch deltas vary
// wildly (5–50 px per event) — an unclamped exponential turns big-delta
// events into runaway zooms. To make pinch feel faster/slower, adjust RATE:
//     0.005 = gentle       0.01 = balanced       0.02 = aggressive
const MAC_ORBIT_RAD_PER_PIXEL = 0.006;
const MAC_PINCH_ZOOM_RATE = 0.01;
const MAC_PINCH_ZOOM_MAX_DELTA = 50;

interface AutoscrollState {
  pointerId: number;
  anchorY: number;
  currentY: number;
}

/**
 * True on macOS (laptop trackpad, Magic Trackpad, Magic Mouse). Detected once
 * at construction and cached — the platform does not change at runtime. Used
 * to switch the wheel handler into Shapr3D-style trackpad mode (orbit by
 * default, pinch to zoom, ⌥ + swipe to pan).
 */
function isMacPlatform(): boolean {
  if (typeof navigator === 'undefined') {
    return false;
  }
  const uaData = navigator as Navigator & { userAgentData?: { platform?: string } };
  const platform = uaData.userAgentData?.platform ?? navigator.platform ?? '';
  const userAgent = navigator.userAgent ?? '';
  // iPadOS reports "MacIntel" but is a touch device — the trackpad wheel
  // model does not apply there (touch handlers run instead).
  if (platform === 'MacIntel' && navigator.maxTouchPoints > 1) {
    return false;
  }
  return /^Mac/i.test(platform) || /Mac OS X/i.test(userAgent);
}

/**
 * Manages OrbitControls configuration, custom orbit inertia, multi-touch
 * gestures (pinch/pan/roll), and Windows-style middle-button autoscroll zoom.
 */
export class SceneControls {
  private orbitInteracting = false;
  private orbitLastSampleTime = 0;
  private orbitLastAzimuth = 0;
  private orbitLastPolar = 0;
  private orbitLastTarget = new Vector3();
  private orbitVelAzimuth = 0;
  private orbitVelPolar = 0;
  private orbitVelTarget = new Vector3();
  /**
   * Snap-breakout state. Tracks how far the pointer has travelled from where the
   * current rotate gesture started; crossing {@link SNAP_BREAKOUT_TRAVEL_PX}
   * fires the release exactly once (`…Emitted`). Reset per pointer drag and per
   * trackpad-swipe burst (idle gap). Pixel travel — not camera angle — is the
   * metric precisely because a held snap does not rotate the camera at all.
   */
  private breakoutStart = new Vector2();
  private breakoutPointerId: number | null = null;
  private breakoutEmitted = false;
  private breakoutWheelPx = 0;
  private breakoutLastWheelTime = 0;
  private autoscroll: AutoscrollState | null = null;
  private readonly raycaster = new Raycaster();
  private readonly ndcScratch = new Vector2();
  private readonly isMac = isMacPlatform();
  private readonly wheelHandler: (event: WheelEvent) => void;

  /**
   * Active while a WebKit trackpad pinch (`gesturestart`…`gestureend`) is in
   * flight. Only ever true in WKWebView (Tauri/macOS). Guards the Chromium
   * `ctrl`+wheel pinch branch so the two engines never double-zoom.
   */
  private gestureActive = false;
  private gestureLastScale = 1;

  /**
   * Action a bare two-finger trackpad swipe performs on macOS. Mirrors
   * `ViewerControl.trackpadTwoFingerGesture`; ⌥ + swipe always does the
   * opposite. Default matches Shapr3D (orbit).
   */
  private twoFingerGesture: 'orbit' | 'pan' = 'orbit';

  /**
   * Fired when a *rotate* gesture breaks a held viewport-cube snap free — a
   * drag/swipe past {@link SNAP_BREAKOUT_TRAVEL_PX}. Used by the auto-ortho
   * behaviour to release the detent and revert the temporary orthographic
   * projection to the user's chosen view.
   *
   * Deliberately **not** fired by pan or zoom: those preserve the dimension-
   * true flat view (you are moving *around* the same look direction, not away
   * from it), which is exactly the sticky behaviour the snap exists to give —
   * e.g. inspecting a sliced face in ortho by panning/zooming around it
   * without ever popping back to perspective. Only actually orbiting to a new
   * look direction counts as leaving the snap. A rotate inside the breakout
   * distance and cube-driven camera moves also deliberately do not fire it,
   * so the snapped view never slips on an accidental nudge. See
   * {@link SceneCamera.notifyUserViewGesture}.
   */
  private revertGestureSink: (() => void) | null = null;

  /** See {@link setPanZoomGestureSink}. */
  private panZoomGestureSink: (() => void) | null = null;

  /** Handlers for the rotate snap-breakout detector, retained for cleanup. */
  private rotateBreakoutPointerDownHandler: ((event: PointerEvent) => void) | null = null;
  private rotateBreakoutPointerMoveHandler: ((event: PointerEvent) => void) | null = null;
  private rotateBreakoutPointerUpHandler: ((event: PointerEvent) => void) | null = null;

  /**
   * Removes the window/document-level safety nets the two-finger controller
   * installs for lifts the canvas never sees. Those outlive the canvas, so they
   * must be torn down explicitly.
   */
  private twoFingerTeardown: (() => void) | null = null;

  /**
   * @param cancelDragCallback  Called when a two-finger gesture begins so
   *   any in-flight single-finger selection drag can be abandoned cleanly.
   */
  constructor(
    private readonly camera: PerspectiveCamera,
    private readonly controls: OrbitControls,
    private readonly renderer: WebGLRenderer,
    private readonly cancelDragCallback: () => void,
  ) {
    // Wheel dispatch is platform-specific. On macOS the wheel channel carries
    // trackpad gestures (two-finger swipe, pinch, ⌥+swipe) and must be
    // interpreted per modifier; on Windows/Linux it is a mouse wheel and only
    // needs zoom.
    this.wheelHandler = this.isMac ? this.onWheelMac : this.onWheel;

    this.installOrbitInertia();
    this.installTouchOrbitTuning();
    this.installCustomTwoFingerControls();
    this.installAutoscrollZoom();
    this.installAlwaysOnWheelZoom();
    this.installWebKitGestureZoom();
    this.installRotateBreakoutDetection();
    // Orbit is the fixed cursor mode: left-drag rotates, right-drag pans.
    // Middle mouse is reserved for autoscroll zoom; disable OrbitControls' drag-dolly.
    const MIDDLE = null as unknown as MOUSE;
    this.controls.mouseButtons = { LEFT: MOUSE.ROTATE, MIDDLE, RIGHT: MOUSE.PAN };
    this.controls.touches = { ONE: TOUCH.ROTATE, TWO: TOUCH_DISABLED };
  }

  hasAutoscroll(): boolean {
    return this.autoscroll !== null;
  }

  /** Set the macOS bare-two-finger-swipe action (orbit or pan). */
  setTwoFingerGesture(gesture: 'orbit' | 'pan'): void {
    this.twoFingerGesture = gesture;
  }

  /**
   * Register a callback fired whenever the user drags a *rotate* gesture far
   * enough to break a viewport-cube snap free (never on a small rotate inside
   * the detent, nor on a cube gesture, nor on any pan/zoom — those are sticky,
   * see {@link setPanZoomGestureSink}). Drives the auto-ortho revert to
   * perspective. Pass `null` to clear.
   */
  setRevertGestureSink(sink: (() => void) | null): void {
    this.revertGestureSink = sink;
  }

  private emitRevertGesture(): void {
    this.revertGestureSink?.();
  }

  /**
   * Register a callback fired on every pan or zoom gesture in the main
   * viewport. Used to release the viewport-cube snap's frozen detent (see
   * {@link SceneCamera.releaseSnapPinForPanZoom}) so the gesture can move the
   * camera — without reverting the temporary orthographic projection, unlike
   * {@link setRevertGestureSink}. Pass `null` to clear.
   */
  setPanZoomGestureSink(sink: (() => void) | null): void {
    this.panZoomGestureSink = sink;
  }

  private emitPanZoomGesture(): void {
    this.panZoomGestureSink?.();
  }

  /** Begin a fresh breakout budget for the next rotate gesture. */
  private resetBreakout(): void {
    this.breakoutWheelPx = 0;
    this.breakoutEmitted = false;
  }

  /**
   * Report how far the pointer has travelled in the current rotate gesture.
   * Fires {@link emitRevertGesture} exactly once, the moment the travel crosses
   * {@link SNAP_BREAKOUT_TRAVEL_PX} — releasing a held snap so the camera can
   * start orbiting. Below that distance nothing is emitted and the snap stays
   * pinned, which is what makes the detent feel sticky.
   */
  private reportBreakoutTravel(travelPx: number): void {
    if (this.breakoutEmitted || travelPx <= SNAP_BREAKOUT_TRAVEL_PX) {
      return;
    }
    this.breakoutEmitted = true;
    this.emitRevertGesture();
  }

  applyOrbitInertia(dt: number): void {
    if (this.orbitInteracting || dt <= 0) {
      return;
    }
    const azSpeed = Math.abs(this.orbitVelAzimuth);
    const polSpeed = Math.abs(this.orbitVelPolar);
    const panSpeed = this.orbitVelTarget.length();
    if (azSpeed < 1e-3 && polSpeed < 1e-3 && panSpeed < 1e-2) {
      this.orbitVelAzimuth = 0;
      this.orbitVelPolar = 0;
      this.orbitVelTarget.set(0, 0, 0);
      return;
    }
    if (azSpeed > 0 || polSpeed > 0) {
      const offset = this.camera.position.clone().sub(this.controls.target);
      const yUp = new Vector3(0, 1, 0);
      const q = new Quaternion().setFromUnitVectors(this.camera.up, yUp);
      const qInv = q.clone().invert();
      offset.applyQuaternion(q);
      const sph = new Spherical().setFromVector3(offset);
      sph.theta += this.orbitVelAzimuth * dt;
      sph.phi += this.orbitVelPolar * dt;
      const eps = 1e-3;
      sph.phi = Math.max(eps, Math.min(Math.PI - eps, sph.phi));
      offset.setFromSpherical(sph).applyQuaternion(qInv);
      this.camera.position.copy(this.controls.target).add(offset);
    }
    if (panSpeed > 0) {
      const dT = this.orbitVelTarget.clone().multiplyScalar(dt);
      this.controls.target.add(dT);
      this.camera.position.add(dT);
    }
    this.camera.lookAt(this.controls.target);
    const halfLifeSeconds = 0.05;
    const decay = Math.pow(0.5, dt / halfLifeSeconds);
    this.orbitVelAzimuth *= decay;
    this.orbitVelPolar *= decay;
    this.orbitVelTarget.multiplyScalar(decay);
  }

  applyAutoscrollZoom(dt: number): void {
    const state = this.autoscroll;
    if (!state || dt <= 0) {
      return;
    }
    const offsetPx = state.anchorY - state.currentY;
    const beyondDeadzone =
      Math.sign(offsetPx) * Math.max(0, Math.abs(offsetPx) - AUTOSCROLL_DEAD_ZONE_PX);
    if (beyondDeadzone === 0) {
      return;
    }
    const accel = Math.pow(
      Math.abs(beyondDeadzone) / AUTOSCROLL_ACCEL_REF_PX,
      AUTOSCROLL_ACCEL_EXPONENT - 1,
    );
    const rate = beyondDeadzone * AUTOSCROLL_SPEED_PER_PX * accel;
    let scale = Math.exp(-rate * dt);
    scale = Math.min(
      AUTOSCROLL_MAX_FACTOR_PER_FRAME,
      Math.max(1 / AUTOSCROLL_MAX_FACTOR_PER_FRAME, scale),
    );
    const target = this.controls.target;
    const offset = this.camera.position.clone().sub(target);
    const oldLen = offset.length();
    if (oldLen < 1e-6) {
      return;
    }
    // Blend proportional zoom with a minimum absolute step (same approach as wheel zoom).
    const proportionalLen = oldLen * scale;
    const minStep = Math.abs(rate * dt) * this.controls.minDistance;
    let newLen: number;
    if (scale < 1) {
      newLen = Math.min(proportionalLen, oldLen - minStep);
    } else {
      newLen = Math.max(proportionalLen, oldLen + minStep);
    }
    newLen = Math.max(this.controls.minDistance, Math.min(this.controls.maxDistance, newLen));
    offset.multiplyScalar(newLen / oldLen);
    this.camera.position.copy(target).add(offset);
  }

  private touchOrbitTuningPointerDownHandler: ((event: PointerEvent) => void) | null = null;
  private touchOrbitTuningPointerMoveHandler: ((event: PointerEvent) => void) | null = null;
  private touchOrbitTuningPointerUpHandler: ((event: PointerEvent) => void) | null = null;
  private touchOrbitTuningPointerCancelHandler: ((event: PointerEvent) => void) | null = null;
  private customTwoFingerPointerDownHandler: ((event: PointerEvent) => void) | null = null;
  private customTwoFingerPointerMoveHandler: ((event: PointerEvent) => void) | null = null;
  private customTwoFingerPointerUpHandler: ((event: PointerEvent) => void) | null = null;
  private customTwoFingerPointerCancelHandler: ((event: PointerEvent) => void) | null = null;

  private uninstallRendererPointerListeners(): void {
    const domElement = this.renderer.domElement;

    if (this.touchOrbitTuningPointerDownHandler) {
      domElement.removeEventListener('pointerdown', this.touchOrbitTuningPointerDownHandler);
      this.touchOrbitTuningPointerDownHandler = null;
    }
    if (this.touchOrbitTuningPointerMoveHandler) {
      domElement.removeEventListener('pointermove', this.touchOrbitTuningPointerMoveHandler);
      this.touchOrbitTuningPointerMoveHandler = null;
    }
    if (this.touchOrbitTuningPointerUpHandler) {
      domElement.removeEventListener('pointerup', this.touchOrbitTuningPointerUpHandler);
      this.touchOrbitTuningPointerUpHandler = null;
    }
    if (this.touchOrbitTuningPointerCancelHandler) {
      domElement.removeEventListener('pointercancel', this.touchOrbitTuningPointerCancelHandler);
      this.touchOrbitTuningPointerCancelHandler = null;
    }

    if (this.customTwoFingerPointerDownHandler) {
      domElement.removeEventListener('pointerdown', this.customTwoFingerPointerDownHandler);
      this.customTwoFingerPointerDownHandler = null;
    }
    if (this.customTwoFingerPointerMoveHandler) {
      domElement.removeEventListener('pointermove', this.customTwoFingerPointerMoveHandler);
      this.customTwoFingerPointerMoveHandler = null;
    }
    if (this.customTwoFingerPointerUpHandler) {
      domElement.removeEventListener('pointerup', this.customTwoFingerPointerUpHandler);
      this.customTwoFingerPointerUpHandler = null;
    }
    if (this.customTwoFingerPointerCancelHandler) {
      domElement.removeEventListener('pointercancel', this.customTwoFingerPointerCancelHandler);
      this.customTwoFingerPointerCancelHandler = null;
    }
  }

  dispose(): void {
    this.uninstallAutoscrollZoom();
    this.uninstallRendererPointerListeners();
    this.uninstallWebKitGestureZoom();
    this.uninstallRotateBreakoutDetection();
    this.twoFingerTeardown?.();
    this.twoFingerTeardown = null;
    this.renderer.domElement.removeEventListener('wheel', this.wheelHandler, { capture: true });
  }

  // -------------------------------------------------------------------------
  // Pointer-drag detection (rotate snap breakout + right-drag pan release)
  // -------------------------------------------------------------------------

  /**
   * Watches raw pointer drags for two purposes that share the same listeners:
   *
   * - **Left-drag / touch / pen = rotate.** Drives the auto-ortho revert (see
   *   {@link revertGestureSink}). Distance from the drag origin is the
   *   snap-breakout metric, measured in *pixels* rather than camera angle
   *   because while a snap is held the camera does not rotate at all
   *   ({@link SceneCamera.applySnapHold}).
   * - **Right-drag = pan.** OrbitControls pans internally (not via our
   *   helpers), so this only needs to release the frozen snap detent (see
   *   {@link panZoomGestureSink}) once the drag genuinely moves — a tiny
   *   travel threshold avoids firing on a bare right-click that never moves —
   *   never the revert-to-perspective sink; pan is sticky.
   *
   * We only observe — never `preventDefault`/`stopPropagation` — so
   * OrbitControls still performs the gesture.
   */
  private installRotateBreakoutDetection(): void {
    const el = this.renderer.domElement;
    let panPointerId: number | null = null;
    let panStartX = 0;
    let panStartY = 0;
    let panEmitted = false;

    this.rotateBreakoutPointerDownHandler = (event: PointerEvent): void => {
      if (event.button === 2) {
        panPointerId = event.pointerId;
        panStartX = event.clientX;
        panStartY = event.clientY;
        panEmitted = false;
        return;
      }
      // Primary button (or a touch/pen contact) starts a rotate: open a fresh
      // breakout budget measured from here. Skipped while `controls` is disabled
      // — that means a gizmo/selection drag (or a snap animation) owns the
      // pointer, and dragging an object must not pop the view out of its snap.
      if (event.button === 0 && this.controls.enabled) {
        this.breakoutPointerId = event.pointerId;
        this.breakoutStart.set(event.clientX, event.clientY);
        this.resetBreakout();
      }
    };
    this.rotateBreakoutPointerMoveHandler = (event: PointerEvent): void => {
      if (event.pointerId === panPointerId && !panEmitted) {
        if (
          Math.hypot(event.clientX - panStartX, event.clientY - panStartY) >
          RIGHT_PAN_RELEASE_THRESHOLD_PX
        ) {
          panEmitted = true;
          this.emitPanZoomGesture();
        }
        return;
      }
      if (event.pointerId === this.breakoutPointerId && this.controls.enabled) {
        this.reportBreakoutTravel(
          Math.hypot(event.clientX - this.breakoutStart.x, event.clientY - this.breakoutStart.y),
        );
      }
    };
    this.rotateBreakoutPointerUpHandler = (event: PointerEvent): void => {
      if (event.pointerId === panPointerId) {
        panPointerId = null;
        panEmitted = false;
      }
      if (event.pointerId === this.breakoutPointerId) {
        this.breakoutPointerId = null;
        this.resetBreakout();
      }
    };

    el.addEventListener('pointerdown', this.rotateBreakoutPointerDownHandler);
    el.addEventListener('pointermove', this.rotateBreakoutPointerMoveHandler);
    el.addEventListener('pointerup', this.rotateBreakoutPointerUpHandler);
    el.addEventListener('pointercancel', this.rotateBreakoutPointerUpHandler);
  }

  private uninstallRotateBreakoutDetection(): void {
    const el = this.renderer.domElement;
    if (this.rotateBreakoutPointerDownHandler) {
      el.removeEventListener('pointerdown', this.rotateBreakoutPointerDownHandler);
      this.rotateBreakoutPointerDownHandler = null;
    }
    if (this.rotateBreakoutPointerMoveHandler) {
      el.removeEventListener('pointermove', this.rotateBreakoutPointerMoveHandler);
      this.rotateBreakoutPointerMoveHandler = null;
    }
    if (this.rotateBreakoutPointerUpHandler) {
      el.removeEventListener('pointerup', this.rotateBreakoutPointerUpHandler);
      el.removeEventListener('pointercancel', this.rotateBreakoutPointerUpHandler);
      this.rotateBreakoutPointerUpHandler = null;
    }
  }

  // -------------------------------------------------------------------------
  // Always-on wheel zoom (bypasses OrbitControls state-machine blocking)
  // -------------------------------------------------------------------------

  /**
   * OrbitControls blocks scroll-wheel zoom when its internal state is PAN
   * or DOLLY (i.e. while a left-drag is in progress in pan/zoom cursor
   * mode). To allow simultaneous left-drag + scroll-wheel zoom we take over
   * wheel handling entirely with a capture-phase listener that directly
   * moves the camera, then stops propagation so OrbitControls never sees
   * the event.
   *
   * On macOS the wheel channel also carries trackpad gestures (two-finger
   * swipe, pinch, ⌥+swipe), so a modifier-aware dispatcher
   * ({@link onWheelMac}) is installed instead of the plain zoom handler.
   */
  private installAlwaysOnWheelZoom(): void {
    this.renderer.domElement.addEventListener('wheel', this.wheelHandler, {
      passive: false,
      capture: true,
    });
  }

  private onWheel = (event: WheelEvent): void => {
    if (!this.controls.enabled || !this.controls.enableZoom || this.autoscroll !== null) {
      return;
    }
    event.preventDefault();
    event.stopImmediatePropagation();
    // Zoom is sticky — it never breaks a held viewport-cube ortho snap (see
    // {@link revertGestureSink}), but it does need to release the frozen
    // detent so the zoom actually shows (see {@link panZoomGestureSink}).
    this.emitPanZoomGesture();

    const zoomSpeed = this.controls.zoomSpeed;
    let normalised: number;
    switch (event.deltaMode) {
      case WheelEvent.DOM_DELTA_LINE:
        normalised = (-event.deltaY * zoomSpeed) / 10;
        break;
      case WheelEvent.DOM_DELTA_PAGE:
        normalised = -event.deltaY * zoomSpeed;
        break;
      default:
        normalised = (-event.deltaY * zoomSpeed) / 480;
        break;
    }
    const target = this.controls.target;
    const offset = this.camera.position.clone().sub(target);
    const oldDist = offset.length();
    if (oldDist < 1e-6) {
      return;
    }
    // Blend proportional zoom with a minimum absolute step scaled to current
    // distance so close-range zoom stays responsive at any zoom level.
    const proportionalDist = oldDist * Math.pow(0.85, normalised);
    const minStep = Math.abs(normalised) * Math.max(this.controls.minDistance, oldDist * 0.1);
    let newDist: number;
    if (normalised > 0) {
      newDist = Math.min(proportionalDist, oldDist - minStep);
    } else if (normalised < 0) {
      newDist = Math.max(proportionalDist, oldDist + minStep);
    } else {
      newDist = oldDist;
    }
    newDist = Math.max(this.controls.minDistance, Math.min(this.controls.maxDistance, newDist));
    const f = newDist / oldDist;
    this.camera.position.copy(target).add(offset.multiplyScalar(f));
  };

  // -------------------------------------------------------------------------
  // WebKit trackpad pinch (Tauri/macOS)
  // -------------------------------------------------------------------------

  /**
   * WKWebView (the Tauri desktop webview on macOS) does not synthesise the
   * `ctrl`+wheel event that Chromium emits for a trackpad pinch — it fires the
   * Safari-proprietary `gesturestart`/`gesturechange`/`gestureend` events
   * instead. Without this handler pinch-to-zoom is silently dead in the desktop
   * app (and the global `gesture*` blocker in index.html would eat the event
   * anyway). We map the cumulative `scale` to the same cursor-anchored dolly the
   * touch pinch uses. WebKit-only, so gating on {@link isMac} is sufficient —
   * Chromium never fires these events, so there is no double-zoom.
   */
  private installWebKitGestureZoom(): void {
    if (!this.isMac) {
      return;
    }
    const el = this.renderer.domElement;
    el.addEventListener('gesturestart', this.onGestureStart as EventListener, { passive: false });
    el.addEventListener('gesturechange', this.onGestureChange as EventListener, { passive: false });
    el.addEventListener('gestureend', this.onGestureEnd as EventListener, { passive: false });
  }

  private uninstallWebKitGestureZoom(): void {
    if (!this.isMac) {
      return;
    }
    const el = this.renderer.domElement;
    el.removeEventListener('gesturestart', this.onGestureStart as EventListener);
    el.removeEventListener('gesturechange', this.onGestureChange as EventListener);
    el.removeEventListener('gestureend', this.onGestureEnd as EventListener);
  }

  private onGestureStart = (event: WebKitGestureEvent): void => {
    if (!this.controls.enabled || !this.controls.enableZoom || this.autoscroll !== null) {
      return;
    }
    event.preventDefault();
    event.stopImmediatePropagation();
    this.gestureActive = true;
    this.gestureLastScale = event.scale || 1;
  };

  private onGestureChange = (event: WebKitGestureEvent): void => {
    if (!this.gestureActive) {
      return;
    }
    event.preventDefault();
    event.stopImmediatePropagation();
    if (!this.controls.enableZoom) {
      return;
    }
    const scale = event.scale || this.gestureLastScale;
    // Pinch open → scale grows → factor < 1 → zoom in (matches applyTouchDolly).
    const factor = this.gestureLastScale / Math.max(scale, 1e-3);
    this.gestureLastScale = scale;
    if (Math.abs(factor - 1) < 1e-4) {
      return;
    }
    this.applyTouchDolly(factor, event.clientX, event.clientY);
  };

  private onGestureEnd = (event: WebKitGestureEvent): void => {
    if (!this.gestureActive) {
      return;
    }
    event.preventDefault();
    event.stopImmediatePropagation();
    this.gestureActive = false;
  };

  // -------------------------------------------------------------------------
  // macOS trackpad wheel dispatch (Shapr3D-style)
  // -------------------------------------------------------------------------
  /**
   * On macOS the `wheel` channel carries every two-finger trackpad gesture,
   * distinguished only by modifier flags. This dispatcher routes each event
   * to the matching camera operation instead of unconditionally zooming.
   *
   * | Modifier                 | Gesture on trackpad          | Action              |
   * | ------------------------ | ---------------------------- | ------------------- |
   * | `ctrlKey` (synthesised)  | Pinch                        | Zoom to cursor      |
   * | none                     | Two-finger swipe             | Primary (orbit/pan) |
   * | `altKey` (⌥ Option)      | Two-finger swipe + Option    | The other one       |
   *
   * The bare-swipe action is user-configurable via {@link setTwoFingerGesture}
   * (default orbit, Shapr3D-style); ⌥ always performs the opposite, so pan is
   * always reachable without the keyboard once the preference is set to pan.
   *
   * `ctrlKey` is set by macOS itself when a pinch gesture is in progress —
   * it does not require the user to press Control. Real Ctrl+scroll on an
   * external mouse also lands here and is treated as pinch, which matches
   * every native macOS app.
   */
  private onWheelMac = (event: WheelEvent): void => {
    if (!this.controls.enabled || this.autoscroll !== null) {
      return;
    }
    event.preventDefault();
    event.stopImmediatePropagation();

    if (event.ctrlKey) {
      // Trackpad pinch (or Ctrl+scroll) — zoom toward the cursor.
      // Direct exponential: positive deltaY (pinch close) → factor > 1 →
      // zoom out; negative deltaY (pinch open) → factor < 1 → zoom in.
      // The per-event clamp keeps a single large-delta event from causing
      // a runaway zoom in the middle of an otherwise-smooth pinch.
      if (!this.controls.enableZoom || this.gestureActive) {
        return;
      }
      const clamped = Math.max(
        -MAC_PINCH_ZOOM_MAX_DELTA,
        Math.min(MAC_PINCH_ZOOM_MAX_DELTA, event.deltaY),
      );
      const factor = Math.exp(clamped * MAC_PINCH_ZOOM_RATE);
      this.applyTouchDolly(factor, event.clientX, event.clientY);
      return;
    }

    // The bare two-finger swipe performs the user's chosen primary gesture
    // (orbit by default, or pan); holding ⌥ performs the other one. Pan uses
    // the touch-pan helper so pixel deltas map 1:1 to world translation
    // (grab feel — the scene follows the fingers).
    const wantOrbit = this.twoFingerGesture === 'orbit' ? !event.altKey : event.altKey;
    if (wantOrbit) {
      this.applyWheelOrbit(event.deltaX, event.deltaY);
    } else {
      this.applyTouchPan(event.deltaX, event.deltaY);
    }
  };

  /**
   * Orbit the camera around `controls.target` by a screen-space pixel delta.
   * Used by the macOS wheel handler for a bare two-finger swipe. Works in the
   * current camera-up frame (reducing to Z-up spherical when level) so it stays
   * consistent with {@link SceneCamera.orbitBy} and with a rolled view.
   */
  private applyWheelOrbit(dxPx: number, dyPx: number): void {
    if (dxPx === 0 && dyPx === 0) {
      return;
    }
    const target = this.controls.target;
    const offset = this.camera.position.clone().sub(target);
    const r = offset.length();
    if (r < 1e-6) {
      return;
    }
    const dAz = dxPx * MAC_ORBIT_RAD_PER_PIXEL;
    const dPol = dyPx * MAC_ORBIT_RAD_PER_PIXEL;

    // Orbit in the camera-up frame: phi = angle from +up, theta = azimuth
    // around +up. Reduces to Z-up spherical when up = (0,0,1).
    const up = this.camera.up.clone().normalize();
    const helper = Math.abs(up.y) < 0.99 ? new Vector3(0, 1, 0) : new Vector3(1, 0, 0);
    const e1 = new Vector3().crossVectors(helper, up).normalize();
    const e2 = new Vector3().crossVectors(up, e1);
    const phi = Math.atan2(Math.hypot(offset.dot(e1), offset.dot(e2)), offset.dot(up));
    const theta = Math.atan2(offset.dot(e2), offset.dot(e1));
    const newTheta = theta - dAz;
    // Clamp phi away from the poles to prevent lookAt degeneracy.
    const eps = 0.01;
    const newPhi = Math.max(eps, Math.min(Math.PI - eps, phi - dPol));

    const sinPhi = Math.sin(newPhi);
    offset
      .copy(up)
      .multiplyScalar(r * Math.cos(newPhi))
      .addScaledVector(e1, r * sinPhi * Math.cos(newTheta))
      .addScaledVector(e2, r * sinPhi * Math.sin(newTheta));
    this.camera.position.copy(target).add(offset);
    // Snap breakout for a trackpad two-finger-swipe rotate (mirrors the pointer
    // rotate path). This channel has no pointer up/down to bracket the gesture,
    // so an idle gap starts a fresh budget and travel is summed from the wheel
    // deltas instead of measured from a drag origin.
    const nowMs = performance.now();
    if (nowMs - this.breakoutLastWheelTime > SNAP_BREAKOUT_IDLE_MS) {
      this.resetBreakout();
    }
    this.breakoutLastWheelTime = nowMs;
    this.breakoutWheelPx += Math.abs(dxPx) + Math.abs(dyPx);
    this.reportBreakoutTravel(this.breakoutWheelPx);
    this.controls.update();
  }

  // -------------------------------------------------------------------------
  // Orbit inertia
  // -------------------------------------------------------------------------

  private installOrbitInertia(): void {
    this.controls.addEventListener('start', () => {
      this.orbitInteracting = true;
      this.orbitLastSampleTime = performance.now();
      this.orbitLastAzimuth = this.controls.getAzimuthalAngle();
      this.orbitLastPolar = this.controls.getPolarAngle();
      this.orbitLastTarget.copy(this.controls.target);
      this.orbitVelAzimuth = 0;
      this.orbitVelPolar = 0;
      this.orbitVelTarget.set(0, 0, 0);
    });
    this.controls.addEventListener('change', () => {
      if (!this.orbitInteracting) {
        return;
      }
      const now = performance.now();
      const dt = (now - this.orbitLastSampleTime) / 1000;
      this.orbitLastSampleTime = now;
      if (dt <= 0 || dt > 0.1) {
        this.orbitLastAzimuth = this.controls.getAzimuthalAngle();
        this.orbitLastPolar = this.controls.getPolarAngle();
        this.orbitLastTarget.copy(this.controls.target);
        return;
      }
      const azNow = this.controls.getAzimuthalAngle();
      const polNow = this.controls.getPolarAngle();
      let dAz = azNow - this.orbitLastAzimuth;
      if (dAz > Math.PI) dAz -= 2 * Math.PI;
      else if (dAz < -Math.PI) dAz += 2 * Math.PI;
      const dPol = polNow - this.orbitLastPolar;
      const dTarget = this.controls.target.clone().sub(this.orbitLastTarget);
      const smoothing = 0.5;
      this.orbitVelAzimuth = lerp(this.orbitVelAzimuth, dAz / dt, smoothing);
      this.orbitVelPolar = lerp(this.orbitVelPolar, dPol / dt, smoothing);
      this.orbitVelTarget.lerp(dTarget.divideScalar(dt), smoothing);
      this.orbitLastAzimuth = azNow;
      this.orbitLastPolar = polNow;
      this.orbitLastTarget.copy(this.controls.target);
    });
    this.controls.addEventListener('end', () => {
      this.orbitInteracting = false;
      const sinceLastSample = (performance.now() - this.orbitLastSampleTime) / 1000;
      if (sinceLastSample > 0.08) {
        this.orbitVelAzimuth = 0;
        this.orbitVelPolar = 0;
        this.orbitVelTarget.set(0, 0, 0);
        return;
      }
      const releaseScale = 0.35;
      this.orbitVelAzimuth *= releaseScale;
      this.orbitVelPolar *= releaseScale;
      this.orbitVelTarget.multiplyScalar(releaseScale);
    });
  }

  // -------------------------------------------------------------------------
  // Touch tuning
  // -------------------------------------------------------------------------

  /**
   * Enable `zoomToCursor` while a touch is active so pinch-zoom converges
   * on the finger centroid, then restores the default (dolly toward target)
   * when the gesture ends.
   */
  private installTouchOrbitTuning(): void {
    const el = this.renderer.domElement;
    const activeTouches = new Set<number>();
    const onDown = (event: PointerEvent): void => {
      if (event.pointerType !== 'touch' || isSyntheticPointerEvent(event)) {
        return;
      }
      activeTouches.add(event.pointerId);
      this.controls.zoomToCursor = true;
    };
    const onEnd = (event: PointerEvent): void => {
      // A synthetic cancel does not mean the finger left — dropping the contact
      // here would clear `zoomToCursor` in the middle of a live pinch.
      if (event.pointerType !== 'touch' || isSyntheticPointerEvent(event)) {
        return;
      }
      activeTouches.delete(event.pointerId);
      if (activeTouches.size === 0) {
        this.controls.zoomToCursor = false;
      }
    };
    el.addEventListener('pointerdown', onDown);
    el.addEventListener('pointerup', onEnd);
    el.addEventListener('pointercancel', onEnd);
  }

  /**
   * Combined pinch-dolly + centroid-pan + twist-roll for two-finger touch.
   * Bypasses OrbitControls entirely while two or more fingers are down.
   *
   * Palm/wrist contacts never reach here while a stylus is in use: the
   * {@link PointerArbiter} installed on the canvas host swallows them in the
   * capture phase, upstream of these listeners. So a resting hand can't be
   * mistaken for the second finger of a pinch. See `scene/pointer-arbiter.ts`.
   *
   * Which of pinch, pan and roll a gesture actually performs is decided by
   * {@link TwoFingerGestureTracker}, not by raw per-frame thresholds — that is
   * what stops a zoom from spinning the view. This method owns only the DOM
   * bookkeeping: which contacts are live, which two of them drive the camera,
   * and how the gesture recovers when the OS never tells us a finger left.
   */
  private installCustomTwoFingerControls(): void {
    const el = this.renderer.domElement;
    const touches = new Map<number, { x: number; y: number; lastSeenMs: number }>();
    const tracker = new TwoFingerGestureTracker(
      {
        engageAngleRad: ROLL_ENGAGE_ANGLE_RAD,
        minSeparationPx: ROLL_MIN_SEPARATION_PX,
        dominanceRatio: ROLL_DOMINANCE_RATIO,
        lockoutPinchRatio: ROLL_LOCKOUT_PINCH_RATIO,
        deadZoneRad: TWO_FINGER_ROLL_DEAD_ZONE_RAD,
        maxStepRad: ROLL_MAX_STEP_RAD,
      },
      {
        deadZonePx: TWO_FINGER_DOLLY_DEAD_ZONE_PX,
        maxStepFactor: DOLLY_MAX_STEP_FACTOR,
        maxPanStepPx: PAN_MAX_STEP_PX,
      },
    );
    const state = {
      active: false,
      savedControlsEnabled: true,
      suppressOwnCancel: false,
      /**
       * The two pointer ids currently driving the camera. Pinned explicitly
       * rather than read off the front of the map each frame, so a third
       * contact cannot quietly become half of the gesture and so a re-anchor
       * happens exactly once, when the pair really changes.
       */
      pair: [] as number[],
    };

    const now = (): number => (typeof performance !== 'undefined' ? performance.now() : Date.now());

    /**
     * Drop contacts whose `pointerup` the OS never delivered — a finger lifted
     * over the toolbar, a pointer stolen mid-gesture, the app backgrounded.
     * Without this the gesture never ends and `controls.enabled` stays `false`,
     * leaving the camera dead until a reload.
     */
    const pruneStaleTouches = (): void => {
      const cutoff = now() - TOUCH_STALE_MS;
      for (const [id, contact] of touches) {
        if (contact.lastSeenMs < cutoff) {
          touches.delete(id);
        }
      }
    };

    const sampleOf = (ids: number[]): TwoFingerSample | null => {
      const a = touches.get(ids[0]);
      const b = touches.get(ids[1]);
      if (!a || !b) {
        return null;
      }
      return {
        dist: Math.hypot(b.x - a.x, b.y - a.y),
        angle: Math.atan2(b.y - a.y, b.x - a.x),
        cx: (a.x + b.x) / 2,
        cy: (a.y + b.y) / 2,
      };
    };

    /** The two longest-standing live contacts — insertion order is arrival order. */
    const currentPair = (): number[] => [...touches.keys()].slice(0, 2);

    const samePair = (next: number[]): boolean =>
      next.length === state.pair.length && next.every((id, i) => id === state.pair[i]);

    /**
     * Re-point the gesture at `next` without emitting motion. The jump between
     * two different pairs of contacts is a bookkeeping artefact, never
     * something the user's hand did.
     */
    const adoptPair = (next: number[], mode: 'begin' | 'reanchor'): void => {
      state.pair = next;
      const sample = sampleOf(next);
      if (!sample) {
        return;
      }
      if (mode === 'begin') {
        tracker.begin(sample);
      } else {
        tracker.reanchor(sample);
      }
    };

    const beginTwoFinger = (): void => {
      state.active = true;
      state.savedControlsEnabled = this.controls.enabled;
      this.controls.enabled = false;
      this.orbitVelAzimuth = 0;
      this.orbitVelPolar = 0;
      this.orbitVelTarget.set(0, 0, 0);
      this.cancelDragCallback();
      // Fire synthetic pointercancel events so OrbitControls clears its
      // internal pointer state before we re-disable it. They are marked so the
      // palm-rejection arbiter and the touch tuning — which model *physical*
      // contacts — ignore them; treating them as real lifts would forget both
      // live fingers and let a palm be admitted into the gesture.
      //
      // Only pointers the element actually holds capture for are cancelled,
      // which is exactly the set OrbitControls has registered: it captures on
      // the `pointerdown` it handles, and the second finger's `pointerdown` is
      // stopped by this very handler, so it never reaches OrbitControls at all.
      // Cancelling that unknown pointer made `releasePointerCapture` throw on
      // every single two-finger gesture — and because that call sits *before*
      // OrbitControls removes its document-level move/up listeners, the throw
      // could leave those attached.
      this.controls.enabled = true;
      state.suppressOwnCancel = true;
      for (const id of touches.keys()) {
        if (!el.hasPointerCapture?.(id)) {
          continue;
        }
        try {
          el.dispatchEvent(
            markSyntheticPointerEvent(
              new PointerEvent('pointercancel', { pointerId: id, pointerType: 'touch' }),
            ),
          );
        } catch {
          // Older browsers may reject the constructor; harmless.
        }
      }
      state.suppressOwnCancel = false;
      this.controls.enabled = false;
      adoptPair(currentPair(), 'begin');
    };

    const endTwoFinger = (): void => {
      if (!state.active) {
        return;
      }
      state.active = false;
      state.pair = [];
      this.controls.enabled = state.savedControlsEnabled;
    };

    const onDown = (event: PointerEvent): void => {
      if (event.pointerType !== 'touch' || isSyntheticPointerEvent(event)) {
        return;
      }
      pruneStaleTouches();
      // If pruning emptied the set, the previous gesture was stranded by a lift
      // the OS never delivered. End it here so this contact starts clean rather
      // than being absorbed into a gesture that no longer has any fingers.
      if (state.active && touches.size === 0) {
        endTwoFinger();
      }
      touches.set(event.pointerId, { x: event.clientX, y: event.clientY, lastSeenMs: now() });
      if (touches.size === 2 && !state.active) {
        beginTwoFinger();
      } else if (state.active) {
        // A third contact does not displace the pair that is already driving.
        if (!samePair(currentPair())) {
          adoptPair(currentPair(), 'reanchor');
        }
      } else {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();
    };

    const onMove = (event: PointerEvent): void => {
      if (event.pointerType !== 'touch' || isSyntheticPointerEvent(event)) {
        return;
      }
      const contact = touches.get(event.pointerId);
      if (!contact) {
        return;
      }
      contact.x = event.clientX;
      contact.y = event.clientY;
      contact.lastSeenMs = now();
      if (!state.active) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();
      if (state.pair.length < 2 || !state.pair.includes(event.pointerId)) {
        return;
      }
      const sample = sampleOf(state.pair);
      if (!sample) {
        return;
      }
      const motion = tracker.update(sample);
      if (motion.dollyFactor !== null) {
        this.applyTouchDolly(motion.dollyFactor, sample.cx, sample.cy);
      }
      if (motion.rollRad !== 0) {
        this.applyTouchRoll(-motion.rollRad);
      }
      if (motion.panDx !== 0 || motion.panDy !== 0) {
        this.applyTouchPan(motion.panDx, motion.panDy);
      }
    };

    /**
     * Retire one contact. Shared by the canvas listeners and the window-level
     * safety net, so a lift that never reaches the canvas still ends the
     * gesture cleanly.
     */
    const releaseTouch = (pointerId: number): boolean => {
      if (!touches.delete(pointerId)) {
        return false;
      }
      if (!state.active) {
        return false;
      }
      if (touches.size === 0) {
        endTwoFinger();
      } else if (!samePair(currentPair())) {
        adoptPair(currentPair(), 'reanchor');
      }
      return true;
    };

    const onUp = (event: PointerEvent): void => {
      if (event.pointerType !== 'touch' || isSyntheticPointerEvent(event)) {
        return;
      }
      if (state.suppressOwnCancel) {
        return;
      }
      if (!releaseTouch(event.pointerId)) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();
    };

    /**
     * Safety net for lifts the canvas never sees. `pointerup` is delivered to
     * whatever element the finger is over, so releasing above the toolbar or a
     * settings panel bypasses the canvas listeners entirely and would otherwise
     * strand the contact.
     */
    const onWindowUp = (event: PointerEvent): void => {
      if (event.pointerType !== 'touch' || isSyntheticPointerEvent(event)) {
        return;
      }
      if (state.suppressOwnCancel || event.target === el) {
        return;
      }
      releaseTouch(event.pointerId);
    };

    /**
     * Abandon the gesture wholesale when the page loses the input stream —
     * backgrounding an iPad mid-pinch delivers no `pointerup` at all.
     */
    const onInterrupt = (): void => {
      touches.clear();
      endTwoFinger();
    };

    el.addEventListener('pointerdown', onDown, { capture: true });
    el.addEventListener('pointermove', onMove, { capture: true });
    el.addEventListener('pointerup', onUp, { capture: true });
    el.addEventListener('pointercancel', onUp, { capture: true });
    window.addEventListener('pointerup', onWindowUp, { capture: true });
    window.addEventListener('pointercancel', onWindowUp, { capture: true });
    window.addEventListener('blur', onInterrupt);
    document.addEventListener('visibilitychange', onInterrupt);

    this.twoFingerTeardown = (): void => {
      window.removeEventListener('pointerup', onWindowUp, { capture: true });
      window.removeEventListener('pointercancel', onWindowUp, { capture: true });
      window.removeEventListener('blur', onInterrupt);
      document.removeEventListener('visibilitychange', onInterrupt);
    };
  }

  // -------------------------------------------------------------------------
  // Touch movement helpers
  // -------------------------------------------------------------------------

  /**
   * Dolly toward / away from the world point under the pinch centroid.
   * `factor < 1` zooms in, `factor > 1` zooms out.
   */
  private applyTouchDolly(factor: number, cx: number, cy: number): void {
    // Zoom is sticky — release the frozen snap detent (never the ortho
    // projection itself) so the dolly actually shows. See
    // {@link panZoomGestureSink}.
    this.emitPanZoomGesture();
    const { camera } = this;
    const target = this.controls.target;
    camera.updateMatrixWorld(true);
    const offset = camera.position.clone().sub(target);
    const oldDist = offset.length();
    if (oldDist < 1e-6) {
      return;
    }
    let newDist = oldDist * factor;
    newDist = Math.max(this.controls.minDistance, Math.min(this.controls.maxDistance, newDist));
    const f = newDist / oldDist;
    if (Math.abs(f - 1) < 1e-6) {
      return;
    }
    const viewNormal = offset.clone().normalize();
    const rect = this.renderer.domElement.getBoundingClientRect();
    const ndcX = ((cx - rect.left) / Math.max(rect.width, 1)) * 2 - 1;
    const ndcY = -(((cy - rect.top) / Math.max(rect.height, 1)) * 2 - 1);
    this.raycaster.setFromCamera(this.ndcScratch.set(ndcX, ndcY), camera);
    const plane = new Plane().setFromNormalAndCoplanarPoint(viewNormal, target);
    const W = new Vector3();
    const hit = this.raycaster.ray.intersectPlane(plane, W);
    camera.position.copy(target).add(offset.multiplyScalar(f));
    if (hit) {
      const shift = W.sub(target).multiplyScalar(1 - f);
      camera.position.add(shift);
      target.add(shift);
    }
  }

  /** Translate camera + target by a screen-space pixel delta. */
  private applyTouchPan(dxPx: number, dyPx: number): void {
    // Pan is sticky — release the frozen snap detent (never the ortho
    // projection itself) so the pan actually shows. See
    // {@link panZoomGestureSink}.
    this.emitPanZoomGesture();
    const { camera } = this;
    const target = this.controls.target;
    camera.updateMatrix();
    const distance = camera.position.distanceTo(target);
    const fovRad = (camera.fov * Math.PI) / 180;
    const viewportHeight = Math.max(this.renderer.domElement.clientHeight, 1);
    const worldPerPixel = (2 * Math.tan(fovRad / 2) * distance) / viewportHeight;
    const right = new Vector3().setFromMatrixColumn(camera.matrix, 0);
    const up = new Vector3().setFromMatrixColumn(camera.matrix, 1);
    const pan = new Vector3();
    pan.addScaledVector(right, -dxPx * worldPerPixel);
    pan.addScaledVector(up, dyPx * worldPerPixel);
    camera.position.add(pan);
    target.add(pan);
  }

  /** Roll the camera by `angle` radians around its forward axis. */
  private applyTouchRoll(angle: number): void {
    const { camera } = this;
    const forward = this.controls.target.clone().sub(camera.position).normalize();
    if (forward.lengthSq() < 1e-12) {
      return;
    }
    const q = new Quaternion().setFromAxisAngle(forward, angle);
    camera.up.applyQuaternion(q).normalize();
    camera.lookAt(this.controls.target);
    camera.updateMatrix();
  }

  // -------------------------------------------------------------------------
  // Autoscroll zoom (Windows-style middle-button hold)
  // -------------------------------------------------------------------------

  private installAutoscrollZoom(): void {
    const el = this.renderer.domElement;
    el.addEventListener('pointerdown', this.onAutoscrollPointerDown);
    el.addEventListener('pointermove', this.onAutoscrollPointerMove);
    el.addEventListener('pointerup', this.onAutoscrollPointerUp);
    el.addEventListener('pointercancel', this.onAutoscrollPointerUp);
    el.addEventListener('contextmenu', this.onAutoscrollContextMenu);
    el.addEventListener('auxclick', this.onAutoscrollAuxClick);
  }

  private uninstallAutoscrollZoom(): void {
    const el = this.renderer.domElement;
    el.removeEventListener('pointerdown', this.onAutoscrollPointerDown);
    el.removeEventListener('pointermove', this.onAutoscrollPointerMove);
    el.removeEventListener('pointerup', this.onAutoscrollPointerUp);
    el.removeEventListener('pointercancel', this.onAutoscrollPointerUp);
    el.removeEventListener('contextmenu', this.onAutoscrollContextMenu);
    el.removeEventListener('auxclick', this.onAutoscrollAuxClick);
  }

  private onAutoscrollPointerDown = (event: PointerEvent): void => {
    if (event.button !== 1) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    // Autoscroll is a zoom gesture — sticky; release the frozen snap detent so
    // the continuous zoom each frame actually shows. See
    // {@link panZoomGestureSink}.
    this.emitPanZoomGesture();
    const el = this.renderer.domElement;
    el.setPointerCapture(event.pointerId);
    el.style.cursor = 'ns-resize';
    this.autoscroll = {
      pointerId: event.pointerId,
      anchorY: event.clientY,
      currentY: event.clientY,
    };
  };

  private onAutoscrollPointerMove = (event: PointerEvent): void => {
    if (!this.autoscroll || event.pointerId !== this.autoscroll.pointerId) {
      return;
    }
    this.autoscroll.currentY = event.clientY;
  };

  private onAutoscrollPointerUp = (event: PointerEvent): void => {
    if (!this.autoscroll || event.pointerId !== this.autoscroll.pointerId) {
      return;
    }
    const el = this.renderer.domElement;
    if (el.hasPointerCapture(event.pointerId)) {
      el.releasePointerCapture(event.pointerId);
    }
    el.style.cursor = '';
    this.autoscroll = null;
  };

  private onAutoscrollContextMenu = (event: Event): void => {
    if (this.autoscroll) {
      event.preventDefault();
    }
  };

  private onAutoscrollAuxClick = (event: MouseEvent): void => {
    if (event.button === 1) {
      event.preventDefault();
    }
  };
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}
