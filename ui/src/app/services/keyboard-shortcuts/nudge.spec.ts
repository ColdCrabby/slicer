import { describe, expect, it } from 'vitest';
import { nudgeDelta } from './nudge';

const UP_Z = { x: 0, y: 0, z: 1 };

describe('nudgeDelta', () => {
  it('moves along the bed axes as seen from the front', () => {
    const front = { x: 0, y: -1, z: 0.5 };
    expect(nudgeDelta('ArrowRight', 1, front, UP_Z)).toEqual([1, 0]);
    expect(nudgeDelta('ArrowLeft', 1, front, UP_Z)).toEqual([-1, 0]);
    expect(nudgeDelta('ArrowUp', 1, front, UP_Z)).toEqual([0, 1]);
    expect(nudgeDelta('ArrowDown', 1, front, UP_Z)).toEqual([0, -1]);
  });

  // From behind the printer the bed's +X is on the left of the screen, so a
  // fixed-axis nudge would push the part the opposite way to the key.
  it('follows the screen, not the bed, from behind', () => {
    const behind = { x: 0, y: 1, z: 0.5 };
    expect(nudgeDelta('ArrowRight', 1, behind, UP_Z)).toEqual([-1, 0]);
    expect(nudgeDelta('ArrowUp', 1, behind, UP_Z)).toEqual([0, -1]);
  });

  it('snaps an angled view to the closest bed axis', () => {
    const [dx, dy] = nudgeDelta('ArrowRight', 10, { x: 0.3, y: -1, z: 0.6 }, UP_Z);
    expect(dx).toBe(10);
    expect(dy).toBe(0);
  });

  it("uses the camera's up when looking straight down", () => {
    const down = { x: 0, y: 0, z: 1 };
    expect(nudgeDelta('ArrowUp', 1, down, { x: 1, y: 0, z: 0 })).toEqual([1, 0]);
  });
});
