import type { FloatingPlacement } from '../../shared/floating';

/**
 * The minimal pointer facts the placement decision needs. Kept DOM-free so the
 * decision is a pure function and unit-testable without a `PointerEvent`.
 */
export interface HoverPointerInfo {
  /** `'mouse' | 'pen' | 'touch'` (or any future pointer type). */
  pointerType: string;
  /**
   * Pen tilt toward +X (right), in degrees, −90…90. Positive means the top of
   * the barrel leans right — i.e. the hand is to the right. Absent/0 for
   * mouse and touch, and for a perfectly upright pen.
   */
  tiltX?: number;
  /** Pen tilt toward +Y (down), in degrees, −90…90. Positive → hand is below. */
  tiltY?: number;
}

/**
 * Below which absolute tilt (deg) a pen is treated as effectively upright, so
 * we stop trusting the tilt direction and fall back to "float above the tip".
 */
const TILT_DEADZONE_DEG = 5;

/**
 * Choose where the G-code inspector tooltip should sit relative to the pointer
 * so the user's hand never covers it.
 *
 * The problem: a fixed below-right placement (great with a mouse) lands the
 * tooltip directly under the palm of a right-handed pen user. The signal that
 * fixes it for free is **pen tilt** — the barrel, and therefore the hand,
 * extends from the tip in the tilt direction, so we place the tooltip on the
 * _opposite_ side. Because tilt reveals which way the pen leans, this adapts to
 * left- vs. right-handed users automatically, with no setting to configure.
 *
 * | Pointer | Placement                                                        |
 * | ------- | ---------------------------------------------------------------- |
 * | mouse   | `right-start` — the familiar below-right desktop behaviour.      |
 * | touch   | `top` — the finger and hand occlude below, so float above.       |
 * | pen     | opposite the tilt (hand) direction; `top` when nearly upright.    |
 *
 * Floating UI's flip/shift still keep the result on-screen near the edges, so
 * this only chooses the _preferred_ side.
 */
export function preferredHoverPlacement(info: HoverPointerInfo): FloatingPlacement {
  if (info.pointerType === 'pen') {
    return penPlacement(info.tiltX ?? 0, info.tiltY ?? 0);
  }
  if (info.pointerType === 'touch') {
    // Finger contact: the fingertip and the hand behind it cover the area below
    // and around the point, so float the readout above it.
    return 'top';
  }
  // Mouse / unknown: keep the desktop-friendly below-and-right placement.
  return 'right-start';
}

function penPlacement(tiltX: number, tiltY: number): FloatingPlacement {
  // Nearly-upright pen (or a device that reports no tilt): we can't tell which
  // way the hand extends, so play it safe and float above the tip.
  if (Math.abs(tiltX) < TILT_DEADZONE_DEG && Math.abs(tiltY) < TILT_DEADZONE_DEG) {
    return 'top';
  }

  // Occlusion (hand) direction ≈ (tiltX, tiltY); we want the opposite.
  const awayX = -tiltX;
  const awayY = -tiltY;

  if (Math.abs(awayX) >= Math.abs(awayY)) {
    // Dominantly horizontal: put the tooltip left/right of the tip, and bias its
    // vertical growth away from the hand — if the hand is below (tiltY > 0) the
    // box should extend upward (`-end`), otherwise downward (`-start`).
    const side = awayX >= 0 ? 'right' : 'left';
    const align = tiltY > 0 ? 'end' : 'start';
    return `${side}-${align}` as FloatingPlacement;
  }

  // Dominantly vertical: put it above/below, biasing horizontal growth away from
  // the hand — hand to the right (tiltX > 0) → box extends left (`-end`).
  const side = awayY >= 0 ? 'bottom' : 'top';
  const align = tiltX > 0 ? 'end' : 'start';
  return `${side}-${align}` as FloatingPlacement;
}
