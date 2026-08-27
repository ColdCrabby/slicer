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

const TOUCH_DISABLED = -1 as unknown as TOUCH;
const TWO_FINGER_DOLLY_DEAD_ZONE_PX = 1.5;
const TWO_FINGER_ROLL_DEAD_ZONE_RAD = 0.01;
/** Right-drag travel (px) before it counts as a pan for the auto-ortho revert. */
const RIGHT_PAN_REVERT_THRESHOLD_PX = 3;

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
   * Fired whenever the user *pans or zooms* the main viewport (never on a
   * rotate). Used by the viewport-cube auto-ortho behaviour to revert the
   * temporary orthographic projection back to the user's chosen view. Rotate
   * and cube-driven camera moves deliberately do not fire it. See
   * {@link SceneCamera.notifyUserPanOrZoom}.
   */
  private revertGestureSink: (() => void) | null = null;

  /** Handlers for the right-drag (pan) revert detector, retained for cleanup. */
  private rightPanPointerDownHandler: ((event: PointerEvent) => void) | null = null;
  private rightPanPointerMoveHandler: ((event: PointerEvent) => void) | null = null;
  private rightPanPointerUpHandler: ((event: PointerEvent) => void) | null = null;

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
    this.installRightPanRevertDetection();
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
   * Register a callback fired whenever the user pans or zooms the main viewport
   * (never on a rotate or a cube gesture). Drives the viewport-cube auto-ortho
   * revert. Pass `null` to clear.
   */
  setRevertGestureSink(sink: (() => void) | null): void {
    this.revertGestureSink = sink;
  }

  private emitRevertGesture(): void {
    this.revertGestureSink?.();
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
    this.uninstallRightPanRevertDetection();
    this.renderer.domElement.removeEventListener('wheel', this.wheelHandler, { capture: true });
  }

  // -------------------------------------------------------------------------
  // Right-drag (pan) revert detection
  // -------------------------------------------------------------------------

  /**
   * OrbitControls maps the right mouse button to PAN. Panning is handled
   * internally by OrbitControls (not by our custom helpers), so to notify the
   * auto-ortho revert we watch the right-button drag ourselves. We only observe
   * — we never call `preventDefault`/`stopPropagation` — so OrbitControls still
   * performs the pan. A small travel threshold avoids reverting on a bare
   * right-click that never moves.
   */
  private installRightPanRevertDetection(): void {
    const el = this.renderer.domElement;
    let pointerId: number | null = null;
    let startX = 0;
    let startY = 0;
    let emitted = false;

    this.rightPanPointerDownHandler = (event: PointerEvent): void => {
      if (event.button !== 2) {
        return;
      }
      pointerId = event.pointerId;
      startX = event.clientX;
      startY = event.clientY;
      emitted = false;
    };
    this.rightPanPointerMoveHandler = (event: PointerEvent): void => {
      if (event.pointerId !== pointerId || emitted) {
        return;
      }
      if (
        Math.hypot(event.clientX - startX, event.clientY - startY) > RIGHT_PAN_REVERT_THRESHOLD_PX
      ) {
        emitted = true;
        this.emitRevertGesture();
      }
    };
    this.rightPanPointerUpHandler = (event: PointerEvent): void => {
      if (event.pointerId === pointerId) {
        pointerId = null;
        emitted = false;
      }
    };

    el.addEventListener('pointerdown', this.rightPanPointerDownHandler);
    el.addEventListener('pointermove', this.rightPanPointerMoveHandler);
    el.addEventListener('pointerup', this.rightPanPointerUpHandler);
    el.addEventListener('pointercancel', this.rightPanPointerUpHandler);
  }

  private uninstallRightPanRevertDetection(): void {
    const el = this.renderer.domElement;
    if (this.rightPanPointerDownHandler) {
      el.removeEventListener('pointerdown', this.rightPanPointerDownHandler);
      this.rightPanPointerDownHandler = null;
    }
    if (this.rightPanPointerMoveHandler) {
      el.removeEventListener('pointermove', this.rightPanPointerMoveHandler);
      this.rightPanPointerMoveHandler = null;
    }
    if (this.rightPanPointerUpHandler) {
      el.removeEventListener('pointerup', this.rightPanPointerUpHandler);
      el.removeEventListener('pointercancel', this.rightPanPointerUpHandler);
      this.rightPanPointerUpHandler = null;
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
    this.emitRevertGesture();

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
      if (event.pointerType !== 'touch') {
        return;
      }
      activeTouches.add(event.pointerId);
      this.controls.zoomToCursor = true;
    };
    const onEnd = (event: PointerEvent): void => {
      if (event.pointerType !== 'touch') {
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
   */
  private installCustomTwoFingerControls(): void {
    const el = this.renderer.domElement;
    const touches = new Map<number, { x: number; y: number }>();
    const state = {
      active: false,
      lastDist: 0,
      lastAngle: 0,
      lastCx: 0,
      lastCy: 0,
      savedControlsEnabled: true,
      suppressOwnCancel: false,
    };

    const recomputeAnchors = (): void => {
      const pts = [...touches.values()];
      if (pts.length < 2) {
        return;
      }
      const a = pts[0];
      const b = pts[1];
      state.lastDist = Math.hypot(b.x - a.x, b.y - a.y);
      state.lastAngle = Math.atan2(b.y - a.y, b.x - a.x);
      state.lastCx = (a.x + b.x) / 2;
      state.lastCy = (a.y + b.y) / 2;
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
      // internal pointer state before we re-disable it.
      this.controls.enabled = true;
      state.suppressOwnCancel = true;
      for (const id of touches.keys()) {
        try {
          el.dispatchEvent(
            new PointerEvent('pointercancel', { pointerId: id, pointerType: 'touch' }),
          );
        } catch {
          // Older browsers may reject the constructor; harmless.
        }
      }
      state.suppressOwnCancel = false;
      this.controls.enabled = false;
      recomputeAnchors();
    };

    const endTwoFinger = (): void => {
      if (!state.active) {
        return;
      }
      state.active = false;
      this.controls.enabled = state.savedControlsEnabled;
    };

    const onDown = (event: PointerEvent): void => {
      if (event.pointerType !== 'touch') {
        return;
      }
      touches.set(event.pointerId, { x: event.clientX, y: event.clientY });
      if (touches.size === 2 && !state.active) {
        beginTwoFinger();
        event.preventDefault();
        event.stopPropagation();
        event.stopImmediatePropagation();
      } else if (state.active) {
        recomputeAnchors();
        event.preventDefault();
        event.stopPropagation();
        event.stopImmediatePropagation();
      }
    };

    const onMove = (event: PointerEvent): void => {
      if (event.pointerType !== 'touch') {
        return;
      }
      const t = touches.get(event.pointerId);
      if (!t) {
        return;
      }
      t.x = event.clientX;
      t.y = event.clientY;
      if (state.active) {
        event.preventDefault();
        event.stopPropagation();
        event.stopImmediatePropagation();
      }
      if (!state.active || touches.size < 2) {
        return;
      }
      const pts = [...touches.values()];
      const a = pts[0];
      const b = pts[1];
      const dist = Math.hypot(b.x - a.x, b.y - a.y);
      const angle = Math.atan2(b.y - a.y, b.x - a.x);
      const cx = (a.x + b.x) / 2;
      const cy = (a.y + b.y) / 2;

      if (state.lastDist > 0 && Math.abs(dist - state.lastDist) > TWO_FINGER_DOLLY_DEAD_ZONE_PX) {
        const factor = state.lastDist / Math.max(dist, 1e-3);
        this.applyTouchDolly(factor, cx, cy);
      }

      let dAngle = angle - state.lastAngle;
      if (dAngle > Math.PI) dAngle -= 2 * Math.PI;
      else if (dAngle < -Math.PI) dAngle += 2 * Math.PI;
      if (Math.abs(dAngle) > TWO_FINGER_ROLL_DEAD_ZONE_RAD) {
        this.applyTouchRoll(-dAngle);
      }

      const dx = cx - state.lastCx;
      const dy = cy - state.lastCy;
      if (dx !== 0 || dy !== 0) {
        this.applyTouchPan(dx, dy);
      }

      state.lastDist = dist;
      state.lastAngle = angle;
      state.lastCx = cx;
      state.lastCy = cy;
    };

    const onUp = (event: PointerEvent): void => {
      if (event.pointerType !== 'touch') {
        return;
      }
      if (state.suppressOwnCancel) {
        return;
      }
      if (!touches.delete(event.pointerId)) {
        return;
      }
      if (!state.active) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();
      if (touches.size === 0) {
        endTwoFinger();
      } else {
        recomputeAnchors();
      }
    };

    el.addEventListener('pointerdown', onDown, { capture: true });
    el.addEventListener('pointermove', onMove, { capture: true });
    el.addEventListener('pointerup', onUp, { capture: true });
    el.addEventListener('pointercancel', onUp, { capture: true });
  }

  // -------------------------------------------------------------------------
  // Touch movement helpers
  // -------------------------------------------------------------------------

  /**
   * Dolly toward / away from the world point under the pinch centroid.
   * `factor < 1` zooms in, `factor > 1` zooms out.
   */
  private applyTouchDolly(factor: number, cx: number, cy: number): void {
    this.emitRevertGesture();
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
    this.emitRevertGesture();
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
    this.emitRevertGesture();
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
