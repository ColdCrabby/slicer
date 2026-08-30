/**
 * Pure, dependency-free slice-progress math — kept out of `slicer.ts` so it can
 * be unit-tested without dragging in Angular or the runtime environment (which
 * touches `window`).
 */

/** One pipeline phase's timing, optionally scoped to an object on the plate. */
export interface PhaseTimingData {
  phase: string;
  startTime?: number;
  endTime?: number;
  elapsedMs?: number;
  /**
   * 1-based object this phase belongs to on a plate sliced object-by-object.
   * Undefined for a single merged slice. Timings are keyed by (phase, object)
   * so object 2 restarting a phase never overwrites object 1's completed entry.
   */
  object?: number;
}

/** Object scope of the current slice: which object of how many is running. */
export interface ObjectScope {
  current: number;
  count: number;
}

/**
 * Proportional weights per phase derived from typical Benchy timings.
 * `total` is the outer span and excluded from progress accumulation.
 */
export const PHASE_WEIGHTS: Record<string, number> = {
  mesh_load: 6,
  mesh_analysis: 1,
  slicing: 46,
  wall_generation: 11,
  infill_region_snapshot: 4,
  wall_restrictions: 7,
  interior_regions: 4,
  surfaces: 8,
  infill: 2,
  gcode_generation: 13,
  file_write: 1,
};

export const PHASE_TOTAL_WEIGHT = Object.values(PHASE_WEIGHTS).reduce((a, b) => a + b, 0);

/**
 * Weighted slice progress (0–100) for a set of phase timings and the current
 * object scope.
 *
 * On a plate sliced object-by-object the pipeline runs once per object, so the
 * result is the fraction of objects already finished plus the weighted fraction
 * of the *current* object's completed phases — never the raw cross-object sum,
 * which jumps backwards each time a new object restarts the pipeline. A merged
 * slice (`count === 1`) reduces to the plain weighted fraction.
 */
export function sliceProgressPercent(timings: PhaseTimingData[], scope: ObjectScope): number {
  const { current, count } = scope;
  const objects = Math.max(1, count);

  let completedWeight = 0;
  for (const t of timings) {
    const belongsToCurrent = (t.object ?? current) === current;
    if (
      t.endTime != null &&
      t.phase !== 'total' &&
      PHASE_WEIGHTS[t.phase] != null &&
      belongsToCurrent
    ) {
      completedWeight += PHASE_WEIGHTS[t.phase];
    }
  }

  const withinObject = completedWeight / PHASE_TOTAL_WEIGHT; // 0..1
  const objectsDone = Math.max(0, current - 1);
  const fraction = (objectsDone + withinObject) / objects;
  return Math.round(fraction * 100);
}
