import { describe, expect, it } from 'vitest';
import { sliceProgressPercent, type ObjectScope, type PhaseTimingData } from './slicer-progress';

/**
 * The multi-object progress bug: slicing a plate object-by-object runs the whole
 * pipeline once per object, so a bar summing completed phase weights across all
 * objects walked *backwards* every time a new object restarted the pipeline.
 * These pin that (a) a single object is unchanged, (b) progress only ever climbs
 * as objects and phases complete, and (c) it reaches ~100% at the end.
 */
const done = (phase: string, object?: number): PhaseTimingData => ({
  phase,
  endTime: 1,
  elapsedMs: 1,
  object,
});

const scope = (current: number, count: number): ObjectScope => ({ current, count });

// A representative slice of the real phase weights (values need not match, only
// that these keys carry weight in PHASE_WEIGHTS).
const EARLY = 'slicing';
const MID = 'wall_generation';
const LATE = 'gcode_generation';

describe('sliceProgressPercent', () => {
  it('is 0 before any phase completes', () => {
    expect(sliceProgressPercent([], scope(1, 1))).toBe(0);
  });

  it('grows monotonically for a single (merged) slice', () => {
    const a = sliceProgressPercent([done(EARLY)], scope(1, 1));
    const b = sliceProgressPercent([done(EARLY), done(MID)], scope(1, 1));
    const c = sliceProgressPercent([done(EARLY), done(MID), done(LATE)], scope(1, 1));
    expect(a).toBeGreaterThan(0);
    expect(b).toBeGreaterThan(a);
    expect(c).toBeGreaterThan(b);
  });

  it('never goes backwards when a second object restarts the pipeline', () => {
    // Object 1 finished several phases…
    const object1Mid = [done(EARLY, 1), done(MID, 1), done(LATE, 1)];
    const atObject1 = sliceProgressPercent(object1Mid, scope(1, 2));

    // …then object 2 begins: its own phases are back to zero, but the scope
    // advanced. Progress must be at least where object 1 left off, not reset.
    const atObject2Start = sliceProgressPercent(object1Mid, scope(2, 2));
    expect(atObject2Start).toBeGreaterThanOrEqual(atObject1);

    // And it keeps climbing as object 2's phases complete.
    const atObject2Mid = sliceProgressPercent(
      [...object1Mid, done(EARLY, 2), done(MID, 2)],
      scope(2, 2),
    );
    expect(atObject2Mid).toBeGreaterThan(atObject2Start);
  });

  it('only counts the current object, so object 2 does not double-count object 1', () => {
    const object1Done = [done(EARLY, 1), done(MID, 1), done(LATE, 1)];
    // At the very start of object 2, only object 1 is fully done → ~50% of a
    // two-object plate, regardless of how many phases object 1 logged.
    const pct = sliceProgressPercent(object1Done, scope(2, 2));
    expect(pct).toBeGreaterThanOrEqual(45);
    expect(pct).toBeLessThanOrEqual(55);
  });

  it('reaches past the halfway plateau once the last object is doing work', () => {
    const object1 = [done(EARLY, 1), done(MID, 1), done(LATE, 1)];
    const object2 = [done(EARLY, 2), done(MID, 2), done(LATE, 2)];
    const pct = sliceProgressPercent([...object1, ...object2], scope(2, 2));
    // The representative phases are a subset of the full weight table, so this
    // won't hit exactly 100; assert it is comfortably past the object-1 plateau.
    expect(pct).toBeGreaterThan(50);
  });
});
