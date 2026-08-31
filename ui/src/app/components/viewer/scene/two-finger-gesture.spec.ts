import { describe, expect, it } from 'vitest';
import {
  type DollyPanGate,
  type RollGate,
  type TwoFingerSample,
  TwoFingerGestureTracker,
  wrapAngle,
} from './two-finger-gesture';

/** Mirrors the production tuning in `controls.ts`. */
const ROLL_GATE: RollGate = {
  engageAngleRad: 0.16,
  minSeparationPx: 70,
  dominanceRatio: 1.6,
  lockoutPinchPx: 24,
  deadZoneRad: 0.01,
  maxStepRad: 0.3,
};

const DOLLY_PAN_GATE: DollyPanGate = {
  deadZonePx: 1.5,
  maxStepFactor: 1.6,
  maxPanStepPx: 160,
};

function tracker(): TwoFingerGestureTracker {
  return new TwoFingerGestureTracker(ROLL_GATE, DOLLY_PAN_GATE);
}

/** Two contacts at `dist` apart, rotated by `angle`, centred at `cx`/`cy`. */
function sample(dist: number, angle = 0, cx = 500, cy = 500): TwoFingerSample {
  return { dist, angle, cx, cy };
}

/**
 * A pinch as a real hand performs it: the separation changes smoothly while
 * the wrist adds a little rotation and the fingertips jitter.
 */
function* pinchFrames(
  from: number,
  to: number,
  frames: number,
  opts: { twistRad?: number; jitterPx?: number } = {},
): Generator<TwoFingerSample> {
  const { twistRad = 0, jitterPx = 0 } = opts;
  for (let i = 1; i <= frames; i++) {
    const t = i / frames;
    const dist = from + (to - from) * t;
    // Deterministic pseudo-jitter, so the test cannot flake.
    const jitter = jitterPx === 0 ? 0 : Math.sin(i * 2.399) * (jitterPx / dist);
    yield sample(dist, twistRad * t + jitter);
  }
}

describe('wrapAngle', () => {
  it('takes the short way round the circle', () => {
    expect(wrapAngle(0.2)).toBeCloseTo(0.2);
    expect(wrapAngle(Math.PI * 1.9)).toBeCloseTo(-Math.PI * 0.1);
    expect(wrapAngle(-Math.PI * 1.9)).toBeCloseTo(Math.PI * 0.1);
  });
});

describe('TwoFingerGestureTracker — a zoom never becomes a spin', () => {
  it('does not roll during a pinch that carries ordinary wrist rotation', () => {
    const t = tracker();
    t.begin(sample(240));
    let totalRoll = 0;
    // 12° of incidental twist across a firm pinch — more than a steady hand adds.
    for (const frame of pinchFrames(240, 90, 60, { twistRad: 0.21, jitterPx: 2 })) {
      totalRoll += Math.abs(t.update(frame).rollRad);
    }
    expect(totalRoll).toBe(0);
    expect(t.isRollLocked()).toBe(true);
  });

  it('does not roll as the fingers close, where angular noise explodes', () => {
    const t = tracker();
    t.begin(sample(200));
    let totalRoll = 0;
    // Pinching to 20px apart: 2px of jitter there reads as 5.7 deg/frame, which
    // is what used to whip the camera round.
    for (const frame of pinchFrames(200, 20, 80, { jitterPx: 2.5 })) {
      totalRoll += Math.abs(t.update(frame).rollRad);
    }
    expect(totalRoll).toBe(0);
  });

  it('latches roll off for the rest of the gesture once a pinch is established', () => {
    const t = tracker();
    t.begin(sample(200));
    for (const frame of pinchFrames(200, 150, 20)) {
      t.update(frame);
    }
    expect(t.isRollLocked()).toBe(true);

    // A deliberate, large twist afterwards must still be refused: the user is
    // mid-zoom, and their wrist turning is not a request to roll the camera.
    let totalRoll = 0;
    for (let i = 1; i <= 40; i++) {
      totalRoll += Math.abs(t.update(sample(150, (i / 40) * 1.2)).rollRad);
    }
    expect(totalRoll).toBe(0);
  });
});

describe('TwoFingerGestureTracker — a deliberate twist still rolls', () => {
  it('engages roll for a sustained twist at a workable separation', () => {
    const t = tracker();
    t.begin(sample(240));
    let totalRoll = 0;
    for (let i = 1; i <= 40; i++) {
      totalRoll += t.update(sample(240, (i / 40) * 0.9)).rollRad;
    }
    expect(t.isRollEngaged()).toBe(true);
    // The twist beyond the engage threshold reaches the camera; the qualifying
    // part deliberately does not, so engaging never jolts the view.
    expect(totalRoll).toBeGreaterThan(0.5);
    expect(totalRoll).toBeLessThan(0.9);
  });

  it('never dumps the qualifying twist as a single jump when roll engages', () => {
    const t = tracker();
    t.begin(sample(240));
    let maxStep = 0;
    for (let i = 1; i <= 40; i++) {
      maxStep = Math.max(maxStep, Math.abs(t.update(sample(240, (i / 40) * 0.9)).rollRad));
    }
    expect(maxStep).toBeLessThanOrEqual(ROLL_GATE.maxStepRad);
    expect(maxStep).toBeLessThan(0.1);
  });

  it('refuses roll when the fingers are too close for the angle to mean anything', () => {
    const t = tracker();
    t.begin(sample(40));
    let totalRoll = 0;
    for (let i = 1; i <= 40; i++) {
      totalRoll += Math.abs(t.update(sample(40, (i / 40) * 1.0)).rollRad);
    }
    expect(t.isRollEngaged()).toBe(false);
    expect(totalRoll).toBe(0);
  });
});

describe('TwoFingerGestureTracker — slow pinches still zoom', () => {
  it('accumulates sub-threshold separation instead of discarding it', () => {
    const t = tracker();
    t.begin(sample(200));
    // 0.5px per event — a deliberate slow pinch on a 120Hz iPad. The old
    // per-frame dead zone threw every one of these away and never zoomed.
    let applied = 0;
    let product = 1;
    for (let i = 1; i <= 60; i++) {
      const motion = t.update(sample(200 + i * 0.5));
      if (motion.dollyFactor !== null) {
        applied++;
        product *= motion.dollyFactor;
      }
    }
    expect(applied).toBeGreaterThan(0);
    // 200 -> 230 is a 1.15x separation, so the camera should dolly by ~1/1.15.
    expect(product).toBeCloseTo(200 / 230, 2);
  });

  it('loses no zoom to rounding across many tiny steps', () => {
    const t = tracker();
    t.begin(sample(100));
    let product = 1;
    for (let i = 1; i <= 400; i++) {
      const motion = t.update(sample(100 + i * 0.25));
      if (motion.dollyFactor !== null) {
        product *= motion.dollyFactor;
      }
    }
    expect(product).toBeCloseTo(100 / 200, 2);
  });
});

describe('TwoFingerGestureTracker — discontinuities are absorbed', () => {
  it('clamps an implausible one-event jump in every channel', () => {
    const t = tracker();
    t.begin(sample(200, 0, 500, 500));
    const motion = t.update(sample(20, 2.5, 900, 100));
    expect(motion.dollyFactor).toBeLessThanOrEqual(DOLLY_PAN_GATE.maxStepFactor);
    expect(motion.dollyFactor).toBeGreaterThanOrEqual(1 / DOLLY_PAN_GATE.maxStepFactor);
    expect(Math.abs(motion.rollRad)).toBeLessThanOrEqual(ROLL_GATE.maxStepRad);
    expect(Math.abs(motion.panDx)).toBeLessThanOrEqual(DOLLY_PAN_GATE.maxPanStepPx);
    expect(Math.abs(motion.panDy)).toBeLessThanOrEqual(DOLLY_PAN_GATE.maxPanStepPx);
  });

  it('emits nothing for the frame a new pair is adopted', () => {
    const t = tracker();
    t.begin(sample(200, 0, 500, 500));
    t.update(sample(190, 0.02, 505, 500));
    // A finger lifts and a third contact takes its place: wildly different
    // geometry that owes nothing to the user's hand moving.
    t.reanchor(sample(60, 1.4, 200, 800));
    const motion = t.update(sample(60, 1.4, 200, 800));
    expect(motion.dollyFactor).toBeNull();
    expect(motion.rollRad).toBe(0);
    expect(motion.panDx).toBe(0);
    expect(motion.panDy).toBe(0);
  });

  it('keeps the pinch verdict across a re-anchor', () => {
    const t = tracker();
    t.begin(sample(200));
    for (const frame of pinchFrames(200, 150, 20)) {
      t.update(frame);
    }
    expect(t.isRollLocked()).toBe(true);
    t.reanchor(sample(240));
    // Swapping fingers must not be a backdoor to the roll the lockout denied.
    let totalRoll = 0;
    for (let i = 1; i <= 40; i++) {
      totalRoll += Math.abs(t.update(sample(240, (i / 40) * 1.2)).rollRad);
    }
    expect(t.isRollLocked()).toBe(true);
    expect(totalRoll).toBe(0);
  });

  it('ignores a degenerate sample rather than dividing by zero', () => {
    const t = tracker();
    t.begin(sample(200));
    expect(t.update(sample(0))).toEqual({
      dollyFactor: null,
      rollRad: 0,
      panDx: 0,
      panDy: 0,
    });
    expect(t.update(sample(Number.NaN)).dollyFactor).toBeNull();
  });
});

describe('TwoFingerGestureTracker — panning', () => {
  it('tracks the centroid one-to-one', () => {
    const t = tracker();
    t.begin(sample(200, 0, 500, 500));
    const motion = t.update(sample(200, 0, 530, 470));
    expect(motion.panDx).toBeCloseTo(30);
    expect(motion.panDy).toBeCloseTo(-30);
  });

  it('reports no pan for a stationary centroid', () => {
    const t = tracker();
    t.begin(sample(200, 0, 500, 500));
    const motion = t.update(sample(180, 0, 500, 500));
    expect(motion.panDx).toBe(0);
    expect(motion.panDy).toBe(0);
  });
});
