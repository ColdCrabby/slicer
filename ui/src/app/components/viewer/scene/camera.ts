import { Box3, type Group, type PerspectiveCamera, Sphere, Vector3 } from 'three';
import type { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import type { PrintAreaConfig } from '../../../services/print-area';
import type { ViewerView } from './types';

const DEFAULT_VIEW_DIR = new Vector3(1, -1, 0.8).normalize();
const DEFAULT_FIT_PADDING = 1.4;
const VIEW_TRANSITION_MS = 600;
const PERSPECTIVE_FOV = 45;
const ORTHO_FOV = 1;
const INITIAL_CAMERA_OFFSET = new Vector3(220, -240, 180);
export const INITIAL_CAMERA_UP = new Vector3(0, 0, 1);
export const INITIAL_PERSPECTIVE_FOV = PERSPECTIVE_FOV;
const CAMERA_NEAR = 0.1;
const CAMERA_FAR = 1_000_000;
const UNIT_X = new Vector3(1, 0, 0);
const UNIT_Y = new Vector3(0, 1, 0);
// A pure top/bottom (±Z) look is degenerate under world Z-up because the view
// axis is parallel to `up`. We snap to a hair off the pole instead of flipping
// `up` sideways, which keeps the orbit frame identical to every side view. ~1°
// is imperceptible yet sits comfortably above OrbitControls' polar clamp.
const POLE_NUDGE_RAD = 0.02;

interface CameraAnimation {
  startTime: number;
  duration: number;
  fromDir: Vector3;
  toDir: Vector3;
  fromFov: number;
  toFov: number;
  fromTarget: Vector3;
  toTarget: Vector3;
  fromUp: Vector3;
  toUp: Vector3;
  fromDistance: number;
  toDistance: number;
}

/**
 * Handles camera positioning, view-preset animations, fit-to-content,
 * and near/far plane management for the viewer scene.
 */
export class SceneCamera {
  private currentView: ViewerView = 'perspective';
  private animation: CameraAnimation | null = null;
  private printArea: PrintAreaConfig;
  /** User-configurable perspective FOV (degrees); the ortho preset ignores it. */
  private perspectiveFov = PERSPECTIVE_FOV;

  constructor(
    private readonly camera: PerspectiveCamera,
    private readonly controls: OrbitControls,
    private readonly contentRoot: Group,
    initialPrintArea: PrintAreaConfig,
  ) {
    this.printArea = { ...initialPrintArea };
  }

  /**
   * Compute the default camera pose relative to the given print area.
   * Called from ViewerScene constructor before SceneCamera exists.
   */
  static computeInitialPose(config: PrintAreaConfig): { position: Vector3; target: Vector3 } {
    const { movableAreaX, movableAreaY, printableAreaWidth, printableAreaHeight } = config;
    const target = new Vector3(
      movableAreaX + printableAreaWidth / 2,
      movableAreaY + printableAreaHeight / 2,
      0,
    );
    return { position: target.clone().add(INITIAL_CAMERA_OFFSET), target };
  }

  setPrintArea(config: PrintAreaConfig): void {
    this.printArea = { ...config };
  }

  /**
   * Set the perspective field-of-view (degrees). When the camera is currently
   * in the perspective preset the change is applied live, adjusting the orbit
   * distance so the framed content keeps its apparent size (a wider FOV would
   * otherwise appear to zoom out). The ortho preset is left untouched — it
   * forces a ~1° FOV to fake an orthographic projection.
   */
  setPerspectiveFov(fov: number): void {
    this.perspectiveFov = fov;
    if (this.currentView !== 'perspective' || this.animation) {
      return;
    }
    const target = this.controls.target;
    const currentDistance = Math.max(this.camera.position.distanceTo(target), 1);
    const fromTan = Math.tan(((this.camera.fov / 2) * Math.PI) / 180);
    const toTan = Math.tan(((fov / 2) * Math.PI) / 180);
    const distance = toTan > 1e-6 ? currentDistance * (fromTan / toTan) : currentDistance;
    const dir = this.camera.position.clone().sub(target).normalize();
    this.camera.position.copy(target).addScaledVector(dir, distance);
    this.camera.fov = fov;
    this.updateNearFar(distance);
    this.camera.updateProjectionMatrix();
    this.controls.update();
  }

  /** Re-frame the camera so the whole content fits comfortably in view. */
  fitToContent(padding = DEFAULT_FIT_PADDING): void {
    const sphere = this.contentBoundingSphere();
    if (!sphere) {
      return;
    }
    const fovRad = (this.camera.fov * Math.PI) / 180;
    const distance = (sphere.radius * padding) / Math.sin(fovRad / 2);
    this.camera.position.copy(sphere.center).addScaledVector(DEFAULT_VIEW_DIR, distance);
    this.controls.target.copy(sphere.center);
    this.updateNearFar(distance, sphere.radius);
    this.camera.updateProjectionMatrix();
    this.controls.update();
  }

  setView(view: ViewerView): void {
    if (view === this.currentView && !this.animation) {
      return;
    }
    this.currentView = view;
    this.animateToView(view);
  }

  resetView(): void {
    this.currentView = 'perspective';
    const pose = this.initialPoseForBed();
    this.animateToPose({
      position: pose.position,
      target: pose.target,
      up: INITIAL_CAMERA_UP.clone(),
      fov: this.perspectiveFov,
    });
  }

  animateToDirection(direction: Vector3, up: Vector3): void {
    const target = this.controls.target.clone();
    const distance = Math.max(this.camera.position.distanceTo(target), 1);
    // Express every snapped view in the scene's stable world Z-up frame so that
    // resuming an orbit afterwards feels identical no matter which face/edge/
    // corner was clicked. A pure ±Z look is degenerate under Z-up (view axis ∥
    // up), so nudge it just off the pole — preserving the *current* azimuth —
    // instead of flipping `up` sideways. The sideways-up was what made orbiting
    // after a Top/Bottom click behave completely differently from the sides.
    let dir = direction.clone().normalize();
    let resolvedUp = up.clone().normalize();
    if (Math.abs(dir.dot(INITIAL_CAMERA_UP)) > 1 - 1e-4) {
      const offset = this.camera.position.clone().sub(target);
      const rawTheta = offset.x === 0 && offset.y === 0 ? 0 : Math.atan2(offset.y, offset.x);
      // Snap the azimuth to a right angle so the Top/Bottom view lands square
      // (face edges parallel to the screen) instead of inheriting whatever
      // arbitrary heading the camera happened to have.
      const theta = Math.round(rawTheta / (Math.PI / 2)) * (Math.PI / 2);
      const sign = dir.z >= 0 ? 1 : -1;
      const sinEps = Math.sin(POLE_NUDGE_RAD);
      dir = new Vector3(
        sinEps * Math.cos(theta),
        sinEps * Math.sin(theta),
        sign * Math.cos(POLE_NUDGE_RAD),
      ).normalize();
      resolvedUp = INITIAL_CAMERA_UP.clone();
    }
    this.animateToPose({
      position: target.clone().addScaledVector(dir, distance),
      target,
      up: resolvedUp,
      fov: this.camera.fov,
    });
  }

  orbitBy(azimuth: number, polar: number): void {
    this.animation = null;
    this.controls.enabled = true;
    const target = this.controls.target;
    const offset = this.camera.position.clone().sub(target);
    const r = offset.length();
    if (r < 1e-6) {
      return;
    }

    // Orbit in the frame defined by the current camera up so the gesture is
    // consistent whether the view is level (up = world Z) or rolled. Reduces
    // exactly to Z-up spherical when up = (0,0,1). We intentionally do not touch
    // camera.up — OrbitControls.update() derives the orientation from it.
    const up = this.camera.up.clone().normalize();
    const helper = Math.abs(up.y) < 0.99 ? UNIT_Y : UNIT_X;
    const e1 = new Vector3().crossVectors(helper, up).normalize();
    const e2 = new Vector3().crossVectors(up, e1);
    const axial = offset.dot(up);
    const rho = Math.hypot(offset.dot(e1), offset.dot(e2));
    const phi = Math.atan2(rho, axial); // angle from +up
    const theta = Math.atan2(offset.dot(e2), offset.dot(e1));

    const newTheta = theta - azimuth;
    // Clamp phi away from the poles to prevent lookAt degeneracy and flicker.
    const eps = 0.01;
    const newPhi = Math.max(eps, Math.min(Math.PI - eps, phi - polar));
    const sinPhi = Math.sin(newPhi);

    offset
      .copy(up)
      .multiplyScalar(r * Math.cos(newPhi))
      .addScaledVector(e1, r * sinPhi * Math.cos(newTheta))
      .addScaledVector(e2, r * sinPhi * Math.sin(newTheta));

    this.camera.position.copy(target).add(offset);
    this.controls.update();
  }

  /**
   * Roll the view about its own axis by `radians` (animated). Rotating up about
   * the view direction rolls the on-screen image; subsequent orbiting stays
   * consistent because orbitBy works in the camera-up frame.
   */
  rollBy(radians: number): void {
    const target = this.controls.target.clone();
    const offset = this.camera.position.clone().sub(target);
    const dir = offset.lengthSq() > 1e-6 ? offset.clone().normalize() : DEFAULT_VIEW_DIR.clone();
    const up = this.camera.up.clone().normalize().applyAxisAngle(dir, radians).normalize();
    this.animateToPose({
      position: this.camera.position.clone(),
      target,
      up,
      fov: this.camera.fov,
    });
  }

  /** Advance an in-flight camera animation one frame. Returns true while animating. */
  advance(): boolean {
    if (!this.animation) {
      return false;
    }
    this.advanceAnimation();
    return this.animation !== null;
  }

  isAnimating(): boolean {
    return this.animation !== null;
  }

  updateNearFar(distance?: number, radius?: number): void {
    const dist =
      distance !== undefined && Number.isFinite(distance) && distance > 0
        ? distance
        : Math.max(this.camera.position.distanceTo(this.controls.target), 1);
    const { printableAreaWidth, printableAreaHeight } = this.printArea;
    const bedRadius = Math.max(printableAreaWidth, printableAreaHeight, 200);
    const sceneRadius = Math.max(radius ?? 0, bedRadius);
    let near = (dist - sceneRadius) * 0.5;
    let far = (dist + sceneRadius) * 4;
    if (!Number.isFinite(near) || near < CAMERA_NEAR) {
      near = CAMERA_NEAR;
    }
    if (!Number.isFinite(far) || far > CAMERA_FAR) {
      far = CAMERA_FAR;
    }
    if (far <= near + 1) {
      far = near + 1;
    }
    near = quantise(near, 0.005);
    far = quantise(far, 0.005);
    if (this.camera.near !== near || this.camera.far !== far) {
      this.camera.near = near;
      this.camera.far = far;
      this.camera.updateProjectionMatrix();
    }
  }

  private initialPoseForBed(): { position: Vector3; target: Vector3 } {
    return SceneCamera.computeInitialPose(this.printArea);
  }

  private contentBoundingSphere(): Sphere | null {
    const box = new Box3().setFromObject(this.contentRoot);
    if (box.isEmpty()) {
      const { movableAreaX, movableAreaY, printableAreaWidth, printableAreaHeight } =
        this.printArea;
      box.set(
        new Vector3(movableAreaX, movableAreaY, 0),
        new Vector3(movableAreaX + printableAreaWidth, movableAreaY + printableAreaHeight, 0),
      );
    }
    const sphere = new Sphere();
    box.getBoundingSphere(sphere);
    if (sphere.radius <= 0 || !Number.isFinite(sphere.radius)) {
      return null;
    }
    sphere.radius = Math.max(sphere.radius, 1);
    return sphere;
  }

  private planView(view: ViewerView): {
    dir: Vector3;
    fov: number;
    target: Vector3;
    up: Vector3;
  } {
    // Preserve the current camera direction and up so the transition is a
    // pure FOV + distance change — no jarring re-orientation.
    const target = this.controls.target.clone();
    const offset = this.camera.position.clone().sub(target);
    const currentDir =
      offset.lengthSq() > 1e-6 ? offset.clone().normalize() : DEFAULT_VIEW_DIR.clone();
    const currentUp = this.camera.up.clone().normalize();
    switch (view) {
      case 'ortho':
        return { dir: currentDir, fov: ORTHO_FOV, target, up: currentUp };
      case 'perspective':
      default:
        return { dir: currentDir, fov: this.perspectiveFov, target, up: currentUp };
    }
  }

  private animateToView(view: ViewerView): void {
    const plan = this.planView(view);
    const currentDistance = Math.max(this.camera.position.distanceTo(this.controls.target), 1);
    // Target distance that preserves apparent size: d × tan(fov/2) = const.
    // advanceAnimation interpolates in apparent-size space, so this ensures
    // fromApparent === toApparent and content never zooms mid-transition.
    const fromTan = Math.tan(((this.camera.fov / 2) * Math.PI) / 180);
    const toTan = Math.tan(((plan.fov / 2) * Math.PI) / 180);
    const toDistance = currentDistance * (fromTan / toTan);
    this.startAnimation({
      toDir: plan.dir,
      toFov: plan.fov,
      toTarget: plan.target,
      toUp: plan.up,
      toDistance,
    });
  }

  private animateToPose(pose: {
    position: Vector3;
    target: Vector3;
    up: Vector3;
    fov: number;
  }): void {
    const offset = pose.position.clone().sub(pose.target);
    const toDistance = offset.length();
    const toDir = toDistance > 1e-6 ? offset.divideScalar(toDistance) : DEFAULT_VIEW_DIR.clone();
    this.startAnimation({
      toDir,
      toFov: pose.fov,
      toTarget: pose.target,
      toUp: pose.up,
      toDistance,
    });
  }

  private startAnimation(spec: {
    toDir: Vector3;
    toFov: number;
    toTarget: Vector3;
    toUp: Vector3;
    toDistance: number;
  }): void {
    const fromTarget = this.controls.target.clone();
    const offset = this.camera.position.clone().sub(fromTarget);
    const fromDistance = offset.length();
    const fromDir =
      fromDistance > 1e-6 ? offset.clone().divideScalar(fromDistance) : DEFAULT_VIEW_DIR.clone();
    const fromUp = this.camera.up.clone().normalize();
    this.controls.enabled = false;
    this.animation = {
      startTime: performance.now(),
      duration: VIEW_TRANSITION_MS,
      fromDir,
      toDir: spec.toDir.clone().normalize(),
      fromFov: this.camera.fov,
      toFov: spec.toFov,
      fromTarget,
      toTarget: spec.toTarget.clone(),
      fromUp,
      toUp: spec.toUp.clone().normalize(),
      fromDistance,
      toDistance: spec.toDistance,
    };
  }

  private advanceAnimation(): void {
    const anim = this.animation;
    if (!anim) {
      return;
    }
    const now = performance.now();
    const t = Math.min(1, (now - anim.startTime) / anim.duration);
    const eased = easeInOutCubic(t);
    const dir = anim.fromDir.clone().lerp(anim.toDir, eased);
    if (dir.lengthSq() < 1e-6) {
      dir.copy(anim.toDir);
    } else {
      dir.normalize();
    }
    const up = anim.fromUp.clone().lerp(anim.toUp, eased);
    if (up.lengthSq() < 1e-6) {
      up.copy(anim.toUp);
    } else {
      up.normalize();
    }
    const fov = lerp(anim.fromFov, anim.toFov, eased);
    // When FOV changes, interpolate in apparent-size space so content never
    // appears to zoom mid-transition:
    //   apparentSize = distance × tan(fov/2)  →  distance = apparent / tan(fov/2)
    // Linearly blending apparent size from start to end is perceptually uniform
    // regardless of how large the FOV change is, and works for both the
    // perspective↔ortho toggle and the home-button reset.
    let distance: number;
    if (anim.fromFov !== anim.toFov) {
      const fromApparent = anim.fromDistance * Math.tan(((anim.fromFov / 2) * Math.PI) / 180);
      const toApparent = anim.toDistance * Math.tan(((anim.toFov / 2) * Math.PI) / 180);
      const apparent = lerp(fromApparent, toApparent, eased);
      distance = apparent / Math.tan(((fov / 2) * Math.PI) / 180);
    } else {
      distance = lerp(anim.fromDistance, anim.toDistance, eased);
    }
    const target = anim.fromTarget.clone().lerp(anim.toTarget, eased);
    this.camera.up.copy(up);
    this.camera.fov = fov;
    this.camera.position.copy(target).addScaledVector(dir, distance);
    this.controls.target.copy(target);
    this.updateNearFar(distance, Math.max(distance * 0.5, 1));
    this.camera.lookAt(target);
    this.camera.updateProjectionMatrix();
    if (t >= 1) {
      this.controls.enabled = true;
      this.controls.update();
      this.animation = null;
    }
  }
}

function quantise(value: number, step: number): number {
  if (value === 0) {
    return 0;
  }
  const scale = Math.abs(value) * step;
  return Math.round(value / scale) * scale;
}

function easeInOutCubic(t: number): number {
  return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}
