import { describe, expect, it } from 'vitest';
import type { FieldDef, SchemaGroup } from './field-def';
import { buildOutline, filterOutline } from './outline';

function field(key: string, title: string, tier?: FieldDef['tier']): FieldDef {
  return { key, title, type: 'number', required: false, tier };
}

const GROUPS: SchemaGroup[] = [
  {
    name: 'Walls',
    fields: [
      field('wall_count', 'Wall Count'),
      field('wall_transition_angle', 'Wall Transition Angle', 'expert'),
    ],
  },
  {
    name: 'Cooling',
    fields: [
      field('fan_speed', 'Fan Speed'),
      field('min_layer_time', 'Min Layer Time', 'advanced'),
    ],
  },
];

const ICONS = { Walls: 'frame', Cooling: 'snow-flake' };

describe('buildOutline', () => {
  it('lists every setting, whatever tier the form keeps it behind', () => {
    const [walls] = buildOutline(GROUPS, ICONS, new Set());
    expect(walls.entries.map((e) => e.key)).toEqual(['wall_count', 'wall_transition_angle']);
    expect(walls.entries[1].tier).toBe('expert');
  });

  it('carries the section icon and counts what deviates from the baseline', () => {
    const [walls, cooling] = buildOutline(GROUPS, ICONS, new Set(['wall_count']));
    expect(walls.icon).toBe('frame');
    expect(walls.modifiedCount).toBe(1);
    expect(walls.entries[0].modified).toBe(true);
    expect(cooling.modifiedCount).toBe(0);
  });

  it('falls back to the raw key when a field has no label', () => {
    const groups: SchemaGroup[] = [
      { name: 'Walls', fields: [{ key: 'wall_seam', type: 'number', required: false }] },
    ];
    expect(buildOutline(groups, {}, new Set())[0].entries[0].title).toBe('wall_seam');
  });
});

describe('filterOutline', () => {
  const outline = buildOutline(GROUPS, ICONS, new Set(['min_layer_time']));

  it('returns everything for a blank query', () => {
    expect(filterOutline(outline, '  ')).toHaveLength(2);
  });

  it('keeps the sections a matching setting lives in, and drops the rest', () => {
    const result = filterOutline(outline, 'fan');
    expect(result.map((s) => s.name)).toEqual(['Cooling']);
    expect(result[0].entries.map((e) => e.key)).toEqual(['fan_speed']);
  });

  it('matches the raw schema key as well as the label', () => {
    expect(filterOutline(outline, 'wall_transition')[0].entries[0].key).toBe(
      'wall_transition_angle',
    );
  });

  it('gives a whole section when the section name itself matches', () => {
    const result = filterOutline(outline, 'cool');
    expect(result[0].entries).toHaveLength(2);
  });

  it('recounts deviations against what is left on screen', () => {
    expect(filterOutline(outline, 'fan')[0].modifiedCount).toBe(0);
    expect(filterOutline(outline, 'min layer')[0].modifiedCount).toBe(1);
  });
});
