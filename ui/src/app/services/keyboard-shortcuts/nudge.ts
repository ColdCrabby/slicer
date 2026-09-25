import type { Vec3 } from '../viewer-control';

export type NudgeKey = 'ArrowUp' | 'ArrowDown' | 'ArrowLeft' | 'ArrowRight';

/**
 * The bed-plane move an arrow key makes, as seen from the camera.
 *
 * "Right" means right *on screen*: from behind the printer the bed's +X is on
 * the left, and an arrow that moved along a fixed axis would push the part the
 * opposite way to the key. So the screen's right and up are laid onto the bed,
 * then snapped to whichever bed axis each is closest to — a nudge always moves
 * a whole step along X or Y, never a diagonal fraction of both.
 *
 * `direction` points from the orbit target toward the camera. Looking straight
 * down it has no footprint on the bed, and the camera's `up` says which way
 * the screen's top points instead.
 */
export function nudgeDelta(
  key: NudgeKey,
  step: number,
  direction: Vec3,
  up: Vec3,
): [number, number] {
  let fx = -direction.x;
  let fy = -direction.y;
  if (Math.hypot(fx, fy) < 1e-3) {
    fx = up.x;
    fy = up.y;
  }
  // Snap "away from the viewer" to the nearest bed axis; right is a quarter
  // turn clockwise from it.
  const [ax, ay] = Math.abs(fx) >= Math.abs(fy) ? [Math.sign(fx) || 1, 0] : [0, Math.sign(fy) || 1];
  const [x, y] = {
    ArrowUp: [ax, ay],
    ArrowDown: [-ax, -ay],
    ArrowRight: [ay, -ax],
    ArrowLeft: [-ay, ax],
  }[key];
  // `+ 0` folds the -0 that negating a zero component leaves behind.
  return [x * step + 0, y * step + 0];
}
