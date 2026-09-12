import { describe, expect, it } from 'vitest';
import processSchema from '../../../schemas/slicer-engine-process-profile-v1.json';
import type { FieldDef } from './field-def';
import { parseSchema } from './schema-parser';
import {
  TIER_ORDER,
  deepestTier,
  deeperOf,
  isFieldInTier,
  isTierAtMost,
  nextTier,
  shallowestTier,
  tierOf,
} from './relevance';

function field(key: string, tier?: FieldDef['tier']): FieldDef {
  return { key, type: 'number', required: false, tier };
}

describe('disclosure tiers', () => {
  it('treats an unannotated field as everyday', () => {
    expect(tierOf(field('layer_height'))).toBe('everyday');
  });

  it('shows everything at or below the revealed tier', () => {
    expect(isFieldInTier(field('a'), 'everyday')).toBe(true);
    expect(isFieldInTier(field('b', 'advanced'), 'everyday')).toBe(false);
    expect(isFieldInTier(field('b', 'advanced'), 'advanced')).toBe(true);
    expect(isFieldInTier(field('c', 'expert'), 'advanced')).toBe(false);
    expect(isFieldInTier(field('c', 'expert'), 'expert')).toBe(true);
  });

  it('reports the deepest tier a group actually contains', () => {
    expect(deepestTier([field('a'), field('b', 'advanced')])).toBe('advanced');
    expect(deepestTier([field('a')])).toBe('everyday');
    expect(deepestTier([field('a', 'expert')])).toBe('expert');
  });

  it('stops offering a deeper step at the bottom', () => {
    expect(nextTier('everyday')).toBe('advanced');
    expect(nextTier('advanced')).toBe('expert');
    expect(nextTier('expert')).toBeNull();
  });
});

describe('group-level tiers', () => {
  it('reports the shallowest tier a group can show', () => {
    // A group with an everyday field has something to show from the start.
    expect(shallowestTier([field('a'), field('b', 'expert')])).toBe('everyday');
    // One with nothing shallower than advanced has an empty everyday view.
    expect(shallowestTier([field('a', 'advanced'), field('b', 'expert')])).toBe('advanced');
    // And an expert-only group cannot be listed until Expert is revealed.
    expect(shallowestTier([field('a', 'expert')])).toBe('expert');
    expect(shallowestTier([])).toBe('everyday');
  });

  it('lists a group once the revealed tier reaches it', () => {
    expect(isTierAtMost('advanced', 'everyday')).toBe(false);
    expect(isTierAtMost('advanced', 'advanced')).toBe(true);
    expect(isTierAtMost('expert', 'advanced')).toBe(false);
    expect(isTierAtMost('everyday', 'expert')).toBe(true);
  });

  it('never lets a section render shallower than the panel around it', () => {
    // A group revealed only because the panel reached Advanced must show its
    // advanced fields, not an empty body.
    expect(deeperOf('everyday', 'advanced')).toBe('advanced');
    expect(deeperOf('expert', 'advanced')).toBe('expert');
  });
});

describe('the engine schema', () => {
  const schema = processSchema as unknown as { $defs: Record<string, Record<string, unknown>> };
  const fields = parseSchema({ ...schema.$defs['SlicingParams'], $defs: schema.$defs }).fields;

  it('only uses tiers the UI knows how to render', () => {
    const unknown = fields.filter((f) => f.tier && !TIER_ORDER.includes(f.tier));
    expect(unknown.map((f) => `${f.key}=${f.tier}`)).toEqual([]);
  });

  /**
   * The everyday view is what a first print depends on. If it grows without
   * anyone noticing, the panel is a wall of settings again and the tiers have
   * stopped doing their job — so the ceiling is asserted rather than assumed.
   */
  it('keeps the default view small enough to read', () => {
    const everyday = fields.filter((f) => tierOf(f) === 'everyday');
    expect(everyday.length).toBeLessThanOrEqual(40);
  });

  it('leaves the decisions a print depends on in the everyday view', () => {
    const everyday = new Set(fields.filter((f) => tierOf(f) === 'everyday').map((f) => f.key));
    for (const key of [
      'layer_height',
      'infill_density',
      'wall_count',
      'support_enabled',
      'nozzle_temp',
      'bed_temp',
      'print_speed',
    ]) {
      expect(everyday.has(key), `${key} should be everyday`).toBe(true);
    }
  });

  /**
   * An expert-only group is only reachable if the disclosure skips the tier that
   * would reveal nothing. `Time estimate` is the live case: the sole such group
   * on the Printer tab, and it was unreachable while the step advanced one tier
   * at a time.
   */
  it('has at least one group reachable only by skipping a tier', () => {
    const byGroup = new Map<string, FieldDef[]>();
    for (const f of fields) {
      const list = byGroup.get(f.group ?? '') ?? [];
      list.push(f);
      byGroup.set(f.group ?? '', list);
    }
    const expertOnly = [...byGroup.entries()].filter(
      ([, groupFields]) => shallowestTier(groupFields) === 'expert',
    );
    expect(expertOnly.map(([name]) => name)).toContain('Time estimate');
  });

  it('demotes the knobs nobody can reason about', () => {
    const byKey = new Map(fields.map((f) => [f.key, f]));
    for (const key of [
      'wall_transition_threshold',
      'wall_transition_filter_distance',
      'bridge_noise_filter_mm',
      'path_tolerance',
    ]) {
      expect(tierOf(byKey.get(key)!), `${key} should be expert`).toBe('expert');
    }
  });
});
