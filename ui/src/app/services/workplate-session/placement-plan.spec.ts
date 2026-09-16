import { describe, expect, it } from 'vitest';
import { placementKey, planPlacements, type PlacedObject } from './placement-plan';

const at = (file_id: string, part_index = 0): PlacedObject => ({ file_id, part_index });

describe('planPlacements', () => {
  it('gives each record the part it was saved against', () => {
    const plan = planPlacements(
      [at('cube'), at('scene', 1)],
      new Map([
        ['cube', 1],
        ['scene', 2],
      ]),
    );

    expect(plan.placements.map((p) => [p.key, p.occurrence])).toEqual([
      ['cube#0', 0],
      ['scene#1', 0],
    ]);
    expect(plan.missing).toBe(0);
  });

  it('numbers repeats of one part so the extras can be duplicated', () => {
    const plan = planPlacements([at('cube'), at('cube'), at('cube')], new Map([['cube', 1]]));

    expect(plan.placements.map((p) => p.occurrence)).toEqual([0, 1, 2]);
    expect(plan.spare).toEqual([]);
  });

  it('marks a part the document never claimed as spare', () => {
    // A 3MF holding three parts, two of which the user deleted before saving.
    const plan = planPlacements([at('scene', 1)], new Map([['scene', 3]]));

    expect(plan.spare).toEqual(['scene#0', 'scene#2']);
    expect(plan.placements).toHaveLength(1);
  });

  it('counts a record as missing when its file could not be resolved', () => {
    const plan = planPlacements([at('cube'), at('gone')], new Map([['cube', 1]]));

    expect(plan.missing).toBe(1);
    expect(plan.placements.map((p) => p.key)).toEqual(['cube#0']);
  });

  it('counts a record as missing when the file no longer holds that part', () => {
    // The user replaced a three-part 3MF with a single-part one under the same
    // handle; the plate must open with what is there rather than throw.
    const plan = planPlacements([at('scene', 2)], new Map([['scene', 1]]));

    expect(plan.missing).toBe(1);
    expect(plan.placements).toEqual([]);
    expect(plan.spare).toEqual(['scene#0']);
  });

  it('keys a part the same way the caller indexes the parsed objects', () => {
    expect(placementKey('cube', 0)).toBe('cube#0');
    expect(placementKey('scene', 2)).toBe('scene#2');
  });
});
