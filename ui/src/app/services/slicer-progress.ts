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
 * Proportional weights per phase, measured as each phase's share of total slice
 * time in a **release** build across the QA corpus (Benchy, Voron cube, filament
 * caddy, hinge) plus a dense 11 MB CAD export. `total` is the outer span and
 * excluded from progress accumulation.
 *
 * These were previously guessed from one Benchy run and badly mis-ranked real
 * work: `slicing` carried weight 46 for a phase that takes ~3% of a slice, while
 * `infill` carried 2 for one that takes ~19% and `wall_generation` 11 for the
 * single most expensive phase at ~41%. The bar therefore sprinted to nearly half
 * in the first fraction of a second and then crawled — and four phases
 * (overhang classification, path ordering, flow compensation, support) had no
 * entry at all, so it froze outright while they ran.
 *
 * Keep every phase the engine emits listed here, even at weight 1: a missing key
 * contributes nothing and reads to the user as a hang.
 */
export const PHASE_WEIGHTS: Record<string, number> = {
  mesh_load: 2,
  mesh_analysis: 1,
  slicing: 3,
  wall_generation: 41,
  infill_region_snapshot: 2,
  wall_restrictions: 3,
  interior_regions: 1,
  surfaces: 18,
  'Overhang Perimeter Classification': 6,
  infill: 19,
  'Support Generation': 25,
  'Path Ordering': 1,
  'Flow Compensation': 1,
  'Bed Adhesion': 1,
  gcode_generation: 5,
  file_write: 1,
};

/**
 * Phases that only run when a feature is switched on.
 *
 * They are counted in the denominator **only once seen**, so a plain slice —
 * the common case — still reaches 100% instead of topping out short by their
 * weight. When one does appear the denominator grows, which would dip the bar;
 * `Slicer.progressFloor` already holds the high-water mark, so it stalls
 * briefly rather than retreating.
 */
const OPTIONAL_PHASES = new Set(['Support Generation', 'Bed Adhesion']);

/** Weight of every phase that always runs — the baseline denominator. */
const CORE_WEIGHT = Object.entries(PHASE_WEIGHTS)
  .filter(([phase]) => !OPTIONAL_PHASES.has(phase))
  .reduce((sum, [, w]) => sum + w, 0);

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
  // Optional phases only join the denominator once this slice actually runs
  // them, so a plain slice is not permanently short by their weight.
  const seenOptional = new Set<string>();
  for (const t of timings) {
    const belongsToCurrent = (t.object ?? current) === current;
    const weight = PHASE_WEIGHTS[t.phase];
    if (weight == null || t.phase === 'total' || !belongsToCurrent) {
      continue;
    }
    if (OPTIONAL_PHASES.has(t.phase)) {
      seenOptional.add(t.phase);
    }
    if (t.endTime != null) {
      completedWeight += weight;
    }
  }

  let denominator = CORE_WEIGHT;
  for (const phase of seenOptional) {
    denominator += PHASE_WEIGHTS[phase];
  }

  const withinObject = completedWeight / Math.max(1, denominator); // 0..1
  const objectsDone = Math.max(0, current - 1);
  const fraction = (objectsDone + withinObject) / objects;
  return Math.round(fraction * 100);
}
