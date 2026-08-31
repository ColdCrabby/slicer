import { describe, expect, it } from 'vitest';
import { clearOffsetX, overlapsXY, type WorldBox } from './placement';

/** Axis-aligned box from XY extents (Z is irrelevant to footprint packing). */
function box(x0: number, y0: number, x1: number, y1: number): WorldBox {
  return [
    [x0, y0, 0],
    [x1, y1, 10],
  ];
}

const SPACING = 4;

describe('overlapsXY', () => {
  it('treats touching edges as clear', () => {
    // ArrangeOnBed packs objects edge-to-edge; if touching counted as an
    // overlap every arranged plate would warn about itself.
    expect(overlapsXY(box(0, 0, 10, 10), box(10, 0, 20, 10))).toBe(false);
  });

  it('detects a genuine overlap', () => {
    expect(overlapsXY(box(0, 0, 10, 10), box(9, 0, 20, 10))).toBe(true);
  });

  it('ignores boxes that only share Z', () => {
    expect(overlapsXY(box(0, 0, 10, 10), box(50, 50, 60, 60))).toBe(false);
  });
});

describe('clearOffsetX', () => {
  it('leaves an already-clear object where it is', () => {
    expect(clearOffsetX(box(100, 0, 110, 10), [box(0, 0, 10, 10)], SPACING)).toBe(0);
  });

  it('does not move the first object on an empty plate', () => {
    expect(clearOffsetX(box(0, 0, 10, 10), [], SPACING)).toBe(0);
  });

  it('moves past a neighbour of the same size', () => {
    const dx = clearOffsetX(box(0, 0, 10, 10), [box(0, 0, 10, 10)], SPACING);
    expect(dx).toBe(14);
  });

  it('clears a neighbour much WIDER than the new object', () => {
    // The regression: stepping by the new object's own width (15 + 4 = 19)
    // lands at x=19, still inside the 150mm-wide neighbour, and the walk
    // used to give up there — leaving the part embedded in the other model.
    const target = box(0, 0, 15, 15);
    const wide = box(0, 0, 150, 100);
    const dx = clearOffsetX(target, [wide], SPACING);

    expect(dx).not.toBeNull();
    expect(dx).toBe(154);
    // The whole point: the resulting position must actually be clear.
    expect(
      overlapsXY(
        [
          [dx! + 0, 0, 0],
          [dx! + 15, 15, 10],
        ],
        wide,
      ),
    ).toBe(false);
  });

  it('clears a chain of wide neighbours', () => {
    const target = box(0, 0, 10, 10);
    const others = [box(0, 0, 100, 50), box(100, 0, 200, 50)];
    const dx = clearOffsetX(target, others, SPACING);

    expect(dx).not.toBeNull();
    for (const other of others) {
      expect(
        overlapsXY(
          [
            [dx! + 0, 0, 0],
            [dx! + 10, 10, 10],
          ],
          other,
        ),
      ).toBe(false);
    }
  });

  it('slots into a gap between two objects rather than always going to the end', () => {
    const target = box(0, 0, 10, 10);
    // Gap from x=54 to x=200 is wide enough for a 10mm part.
    const others = [box(0, 0, 50, 50), box(200, 0, 250, 50)];
    const dx = clearOffsetX(target, others, SPACING);

    expect(dx).toBe(54);
  });

  it('ignores neighbours that do not share the target Y band', () => {
    const dx = clearOffsetX(box(0, 0, 10, 10), [box(0, 500, 100, 600)], SPACING);
    expect(dx).toBe(0);
  });
});
