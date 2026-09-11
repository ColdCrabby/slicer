import { describe, expect, it } from 'vitest';
import processSchema from '../../../schemas/slicer-engine-process-profile-v1.json';
import type { FieldDef } from './field-def';
import { parseSchema } from './schema-parser';
import { TIER_ORDER, deepestTier, isFieldInTier, nextTier, tierOf } from './relevance';

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
