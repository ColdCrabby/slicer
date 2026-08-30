/** World-space axis-aligned box: `[[minX,minY,minZ], [maxX,maxY,maxZ]]`. */
export type WorldBox = [[number, number, number], [number, number, number]];

/**
 * How far right a new object must move to clear everything already placed.
 *
 * Walks along +X, each pass jumping past the far edge of whichever object is
 * still in the way. Stepping by the *new* object's own width instead would not
 * be enough: a neighbour wider than the new object would still overlap after a
 * step, leaving the object sitting inside it.
 *
 * Returns `0` when the object already has a clear spot, and `null` when no
 * gap was found — the caller should leave the object alone and let the
 * placement warning flag the overlap rather than fling it off the bed.
 */
export function clearOffsetX(
  target: WorldBox,
  others: readonly WorldBox[],
  spacing: number,
): number | null {
  let dx = 0;
  // Each pass fully clears the blocking object, so it terminates in at most
  // one pass per neighbour.
  for (let attempt = 0; attempt <= others.length; attempt++) {
    const box = shiftedX(target, dx);
    const blocker = others.find((other) => overlapsXY(box, other));
    if (!blocker) {
      return dx;
    }
    dx = blocker[1][0] + spacing - target[0][0];
  }
  return null;
}

/** A box shifted along X by `dx`. */
export function shiftedX(box: WorldBox, dx: number): WorldBox {
  const [min, max] = box;
  return [
    [min[0] + dx, min[1], min[2]],
    [max[0] + dx, max[1], max[2]],
  ];
}

/** Do two boxes share XY space? Touching edges do not count. */
export function overlapsXY(a: WorldBox, b: WorldBox): boolean {
  const [aMin, aMax] = a;
  const [bMin, bMax] = b;
  return aMin[0] < bMax[0] && bMin[0] < aMax[0] && aMin[1] < bMax[1] && bMin[1] < aMax[1];
}

/** World-space width/depth of a box. */
export function footprintOf(box: WorldBox): [number, number] {
  const [min, max] = box;
  return [max[0] - min[0], max[1] - min[1]];
}
