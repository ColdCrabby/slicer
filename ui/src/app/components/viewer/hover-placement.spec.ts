import { describe, expect, it } from 'vitest';
import { preferredHoverPlacement } from './hover-placement';

describe('preferredHoverPlacement', () => {
  it('keeps the below-right placement for a mouse', () => {
    expect(preferredHoverPlacement({ pointerType: 'mouse' })).toBe('right-start');
  });

  it('floats above the contact for touch', () => {
    expect(preferredHoverPlacement({ pointerType: 'touch' })).toBe('top');
  });

  it('floats above the tip for a near-upright pen (no usable tilt)', () => {
    expect(preferredHoverPlacement({ pointerType: 'pen', tiltX: 0, tiltY: 0 })).toBe('top');
    expect(preferredHoverPlacement({ pointerType: 'pen', tiltX: 2, tiltY: -3 })).toBe('top');
  });

  it('places opposite the hand for a right-handed pen (leaning down-right)', () => {
    // Hand down-right → tooltip should go left and extend upward (`left-end`).
    const p = preferredHoverPlacement({ pointerType: 'pen', tiltX: 40, tiltY: 20 });
    expect(p).toBe('left-end');
  });

  it('places opposite the hand for a left-handed pen (leaning down-left)', () => {
    // Hand down-left → tooltip should go right and extend upward (`right-end`).
    const p = preferredHoverPlacement({ pointerType: 'pen', tiltX: -40, tiltY: 20 });
    expect(p).toBe('right-end');
  });

  it('uses a vertical side when tilt is dominantly vertical', () => {
    // Hand mostly below, slightly right → float above, biased left (`top-end`).
    expect(preferredHoverPlacement({ pointerType: 'pen', tiltX: 10, tiltY: 60 })).toBe('top-end');
    // Hand mostly above, slightly left → float below, biased right (`bottom-start`).
    expect(preferredHoverPlacement({ pointerType: 'pen', tiltX: -10, tiltY: -60 })).toBe(
      'bottom-start',
    );
  });

  it('biases horizontal placement vertically away from the hand', () => {
    // Hand up-right (tiltY < 0) → tooltip left and extends downward (`left-start`).
    expect(preferredHoverPlacement({ pointerType: 'pen', tiltX: 40, tiltY: -20 })).toBe(
      'left-start',
    );
  });

  it('treats missing tilt as zero (upright) rather than throwing', () => {
    expect(preferredHoverPlacement({ pointerType: 'pen' })).toBe('top');
  });

  it('falls back to the mouse placement for an unknown pointer type', () => {
    expect(preferredHoverPlacement({ pointerType: 'unknown' })).toBe('right-start');
  });
});
