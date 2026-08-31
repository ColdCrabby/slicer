/**
 * Arbitration for two-finger viewport gestures: pinch-dolly, centroid pan and
 * twist-roll.
 *
 * The hard part of a multi-touch camera is not computing the three channels —
 * that is trigonometry — it is deciding *which one the user meant*. Fingers
 * never move on a clean radial line, so a pinch always carries some rotation
 * and a twist always carries some separation change. Feeding all three raw
 * signals straight to the camera makes a zoom spin the model, which is exactly
 * the failure this module exists to remove.
 *
 * Two properties do the work:
 *
 * **Roll must earn its way in, and can be shut out.** Rotation is measured as
 * an *angle*, so its noise floor scales with `1 / separation`: 2 px of tremor
 * reads as 0.6° at 200 px apart but 4.6° at 25 px apart. Since pinching in
 * drives separation down, a fixed angular threshold gets easier to trip exactly
 * as the user zooms — the old behaviour, and the reported bug. So roll engages
 * only after a sustained twist ({@link RollGate.engageAngleRad}) at a
 * separation where the angle means something, and only while tangential travel
 * clearly dominates radial travel. Once radial travel passes
 * {@link RollGate.lockoutPinchPx} the gesture is a pinch **for good** and roll
 * is latched off — a guarantee, not a threshold a jittery frame can beat.
 *
 * **Travel is measured as net displacement, never as accumulated path length.**
 * This is the difference between a gate that works on real hardware and one
 * that cannot fire at all. A fingertip's reported separation jitters every
 * event, so summing `|Δdist|` integrates the *absolute value* of that noise: it
 * only ever grows, and at 120 Hz even 0.3 px of jitter accumulates past a 24 px
 * pinch threshold in **0.58 s** — locking roll out mid-twist, from noise alone,
 * before the user has rotated far enough to engage. Measuring from the
 * gesture's origin instead lets the noise cancel: the same pure twist reads
 * 0.4 px of radial movement against 43.6 px of tangential. Both channels are
 * compared in pixels of real fingertip movement, so the ratio holds at any
 * separation.
 *
 * **Dead zones accumulate, they do not discard.** Each channel keeps its own
 * anchor and only moves it when it actually applies motion, so sub-threshold
 * movement is *stored* rather than thrown away. The previous code re-based
 * every anchor each event, which silently deleted any motion below the
 * threshold; on a 120 Hz iPad a deliberate slow pinch never accumulated the
 * 1.5 px per event it needed and the camera simply refused to zoom.
 *
 * The tracker is pure and clock-free: feed it samples, get motion back. That
 * keeps the arbitration testable without a DOM, a canvas, or a camera.
 */

/** Geometry of the two contacts driving the gesture, in CSS pixels. */
export interface TwoFingerSample {
  /** Distance between the two contacts. */
  dist: number;
  /** Angle of the line joining them, radians, from `Math.atan2`. */
  angle: number;
  /** Centroid x. */
  cx: number;
  /** Centroid y. */
  cy: number;
}

/** Camera motion to apply for one update. Zero/`null` fields mean "no motion". */
export interface TwoFingerMotion {
  /** Dolly ratio (`< 1` zooms in), or `null` when below the dead zone. */
  dollyFactor: number | null;
  /** Roll about the view axis, radians. `0` when roll is not engaged. */
  rollRad: number;
  /** Centroid pan, pixels. */
  panDx: number;
  panDy: number;
}

/** Tuning for the roll gate. See the module doc for the reasoning. */
export interface RollGate {
  /** Net twist from the gesture origin needed before roll engages. */
  engageAngleRad: number;
  /** Separation below which a twist reading is noise by construction. */
  minSeparationPx: number;
  /** Required ratio of net tangential to net radial fingertip displacement. */
  dominanceRatio: number;
  /** Net radial displacement that latches roll off for the rest of the gesture. */
  lockoutPinchPx: number;
  /** Fine-grain dead zone applied once roll is engaged. */
  deadZoneRad: number;
  /** Largest roll applied from one update. */
  maxStepRad: number;
}

/** Tuning for the dolly and pan channels. */
export interface DollyPanGate {
  /** Accumulated separation change before a dolly is applied. */
  deadZonePx: number;
  /** Largest dolly ratio applied from one update. */
  maxStepFactor: number;
  /** Largest centroid pan applied from one update. */
  maxPanStepPx: number;
}

const NO_MOTION: TwoFingerMotion = { dollyFactor: null, rollRad: 0, panDx: 0, panDy: 0 };

/** Shortest signed difference between two angles, in `(-π, π]`. */
export function wrapAngle(delta: number): number {
  let d = delta;
  while (d > Math.PI) {
    d -= 2 * Math.PI;
  }
  while (d < -Math.PI) {
    d += 2 * Math.PI;
  }
  return d;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

/**
 * Turns a stream of two-finger samples into camera motion, arbitrating between
 * pinch, pan and twist. One instance per gesture; call {@link begin} when the
 * second finger lands and {@link reanchor} whenever the pair changes identity.
 */
export class TwoFingerGestureTracker {
  /** Per-channel anchors — moved only when that channel emits motion. */
  private dollyAnchorDist = 0;
  private rollAnchorAngle = 0;
  private panAnchorCx = 0;
  private panAnchorCy = 0;

  /** Previous raw angle, used to accumulate net twist across the gesture. */
  private prevAngle = 0;

  /** Separation when the gesture (or the current pair) began. */
  private originDist = 0;

  /** Net signed twist since the gesture began. */
  private netAngleRad = 0;

  private rollEngaged = false;
  private rollLocked = false;

  constructor(
    private readonly rollGate: RollGate,
    private readonly dollyPanGate: DollyPanGate,
  ) {}

  /** Start a fresh gesture anchored at `sample`. Clears all arbitration state. */
  begin(sample: TwoFingerSample): void {
    this.setAnchors(sample);
    this.resetArbitrationOrigin(sample);
    this.rollEngaged = false;
    this.rollLocked = false;
  }

  /**
   * Move every anchor to `sample` without emitting motion, for when the pair
   * changes identity (a finger lifts, another is adopted). The discontinuity is
   * absorbed instead of being applied as a huge one-frame jump.
   *
   * The arbitration *origin* is re-based too: distances between two different
   * pairs of contacts are not comparable, and carrying the old one over would
   * read the swap as a huge pinch and latch roll off. The *verdicts* however
   * deliberately survive — a gesture already judged a pinch stays a pinch, so
   * swapping fingers is never a backdoor to the roll the lockout just denied.
   */
  reanchor(sample: TwoFingerSample): void {
    this.setAnchors(sample);
    this.resetArbitrationOrigin(sample);
  }

  /** True once the gesture has been judged a twist. Exposed for tests. */
  isRollEngaged(): boolean {
    return this.rollEngaged;
  }

  /** True once the gesture has been latched as a pinch. Exposed for tests. */
  isRollLocked(): boolean {
    return this.rollLocked;
  }

  private setAnchors(sample: TwoFingerSample): void {
    this.dollyAnchorDist = sample.dist;
    this.rollAnchorAngle = sample.angle;
    this.panAnchorCx = sample.cx;
    this.panAnchorCy = sample.cy;
    this.prevAngle = sample.angle;
  }

  /** Re-base the reference the net-displacement measurements are taken from. */
  private resetArbitrationOrigin(sample: TwoFingerSample): void {
    this.originDist = sample.dist;
    this.prevAngle = sample.angle;
    this.netAngleRad = 0;
  }

  /**
   * Accumulate net twist and decide whether the gesture is a twist.
   *
   * Both measurements are **net displacement from the origin**, never summed
   * per-event travel — see the module doc for why that distinction decides
   * whether roll can fire at all on real hardware.
   */
  private arbitrate(sample: TwoFingerSample): void {
    this.netAngleRad += wrapAngle(sample.angle - this.prevAngle);
    this.prevAngle = sample.angle;

    if (this.rollEngaged || this.rollLocked) {
      return;
    }

    // How far the fingers have genuinely separated, and how far they have
    // genuinely swept round. Jitter cancels in both instead of piling up.
    const netRadialPx = Math.abs(sample.dist - this.originDist);
    const netTangentialPx = Math.abs(this.netAngleRad) * (sample.dist / 2);

    if (netRadialPx > this.rollGate.lockoutPinchPx) {
      this.rollLocked = true;
      return;
    }
    const twistedEnough = Math.abs(this.netAngleRad) >= this.rollGate.engageAngleRad;
    const wideEnough = sample.dist >= this.rollGate.minSeparationPx;
    const dominant = netTangentialPx > netRadialPx * this.rollGate.dominanceRatio;
    if (twistedEnough && wideEnough && dominant) {
      this.rollEngaged = true;
    }
  }

  /**
   * Fold one sample into the gesture and report the camera motion it earns.
   */
  update(sample: TwoFingerSample): TwoFingerMotion {
    if (!Number.isFinite(sample.dist) || sample.dist <= 0) {
      return NO_MOTION;
    }
    this.arbitrate(sample);

    const motion: TwoFingerMotion = { dollyFactor: null, rollRad: 0, panDx: 0, panDy: 0 };

    if (Math.abs(sample.dist - this.dollyAnchorDist) > this.dollyPanGate.deadZonePx) {
      const raw = this.dollyAnchorDist / sample.dist;
      motion.dollyFactor = clamp(
        raw,
        1 / this.dollyPanGate.maxStepFactor,
        this.dollyPanGate.maxStepFactor,
      );
      this.dollyAnchorDist = sample.dist;
    }

    if (this.rollEngaged) {
      const twist = wrapAngle(sample.angle - this.rollAnchorAngle);
      if (Math.abs(twist) > this.rollGate.deadZoneRad) {
        motion.rollRad = clamp(twist, -this.rollGate.maxStepRad, this.rollGate.maxStepRad);
        this.rollAnchorAngle = sample.angle;
      }
    } else {
      // Track the angle while roll is denied so engaging later starts from the
      // current pose instead of dumping the whole accumulated twist at once.
      this.rollAnchorAngle = sample.angle;
    }

    const { maxPanStepPx } = this.dollyPanGate;
    motion.panDx = clamp(sample.cx - this.panAnchorCx, -maxPanStepPx, maxPanStepPx);
    motion.panDy = clamp(sample.cy - this.panAnchorCy, -maxPanStepPx, maxPanStepPx);
    this.panAnchorCx = sample.cx;
    this.panAnchorCy = sample.cy;

    return motion;
  }
}
