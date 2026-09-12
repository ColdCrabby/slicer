import { describe, expect, it } from 'vitest';
import processSchema from '../../../schemas/slicer-engine-process-profile-v1.json';
import type { FieldDef } from './field-def';
import { ENUM_LABELS, FIELD_LABELS, fieldLabel } from './field-labels';
import {
  displayUnitOf,
  familyOptions,
  offersUnit,
  switchableFamilies,
  toDisplay,
  toStored,
  unitForField,
} from './field-units';
import { parseSchema } from './schema-parser';

function field(key: string, extra: Partial<FieldDef> = {}): FieldDef {
  return { key, type: 'number', required: false, ...extra };
}

describe('unitForField', () => {
  it('reads the unit the schema declares', () => {
    expect(unitForField(field('a', { unit: 'mm_s' })).unit).toBe('mm/s');
    expect(unitForField(field('b', { unit: 'celsius' })).unit).toBe('°C');
    expect(unitForField(field('c', { unit: 'mm3_s' })).unit).toBe('mm³/s');
  });

  it('shows a fraction as a percentage, and says by how much to scale it', () => {
    // The engine stores `fan_speed: 1.0` for full speed; a box suffixed `%`
    // showing `1` would be saying something else entirely.
    expect(unitForField(field('fan_speed', { unit: 'fraction' }))).toEqual({
      unit: '%',
      step: 5,
      scale: 100,
    });
  });

  it('offers no unit switch on a percentage, only the ×100', () => {
    // `proportion` is a family so the scale lives in one table, not so the user
    // can ask to read a fan speed as `0.6`.
    expect(unitForField(field('fan_speed', { unit: 'fraction' })).options).toBeUndefined();
  });

  it('reads a speed in mm/s whatever the engine stores it in', () => {
    // The whole point: `travel_speed_mm_min` is 9000 on the wire because that is
    // what an `F` word carries, and 150 mm/s to everyone who prints.
    const travel = unitForField(field('travel_speed_mm_min', { unit: 'mm_min' }));
    expect(travel.unit).toBe('mm/s');
    expect(toDisplay(9000, travel.scale)).toBe(150);
    expect(toStored(150, travel.scale)).toBe(9000);

    const print = unitForField(field('print_speed', { unit: 'mm_s' }));
    expect(print.unit).toBe('mm/s');
    expect(print.scale).toBe(1);
  });

  it('switches every speed field together when the user picks mm/min', () => {
    const display = { speed: 'mm_min' };
    const travel = unitForField(field('travel_speed_mm_min', { unit: 'mm_min' }), display);
    const print = unitForField(field('print_speed', { unit: 'mm_s' }), display);

    expect(travel.unit).toBe('mm/min');
    expect(travel.scale).toBe(1);
    expect(print.unit).toBe('mm/min');
    expect(toDisplay(120, print.scale)).toBe(7200);
    expect(toStored(7200, print.scale)).toBe(120);
  });

  it('names the family a speed field can switch, and what it may switch to', () => {
    const speed = unitForField(field('print_speed', { unit: 'mm_s' }));
    expect(speed.family).toBe('speed');
    expect(speed.options).toEqual([
      { id: 'mm_s', label: 'mm/s' },
      { id: 'mm_min', label: 'mm/min' },
    ]);
  });

  it('falls back to the family default when the stored preference is nonsense', () => {
    expect(unitForField(field('print_speed', { unit: 'mm_s' }), { speed: 'furlongs' }).unit).toBe(
      'mm/s',
    );
    expect(displayUnitOf('speed', {})).toBe('mm_s');
    expect(offersUnit('speed', 'mm_min')).toBe(true);
    expect(offersUnit('speed', 'fraction')).toBe(false);
  });

  it('carries an explicit step into the unit the user reads', () => {
    // `retract_speed_mm_min` declares a 60 mm/min step, which is 1 mm/s.
    const f = field('retract_speed_mm_min', { unit: 'mm_min', step: 60 });
    expect(unitForField(f).step).toBe(1);
    expect(unitForField(f, { speed: 'mm_min' }).step).toBe(60);
  });

  it('survives a round trip that does not divide evenly', () => {
    // 7201 mm/min is 120.0166… mm/s. Storing that at the display precision
    // would show the user back `7201.00002` in the box they typed `7201` into.
    const { scale } = unitForField(field('print_speed', { unit: 'mm_s' }), { speed: 'mm_min' });
    expect(toDisplay(toStored(7201, scale), scale)).toBe(7201);

    const travel = unitForField(field('travel_speed_mm_min', { unit: 'mm_min' }));
    expect(toStored(150.5, travel.scale)).toBe(9030);
    expect(toDisplay(9030, travel.scale)).toBe(150.5);
  });

  it('only calls a family switchable when it has something to switch to', () => {
    expect(switchableFamilies()).toEqual(['speed']);
    expect(familyOptions('proportion')).toEqual([{ id: 'percent', label: '%' }]);
  });

  it('distinguishes a percentage from a multiplier', () => {
    expect(unitForField(field('a', { unit: 'percent' })).unit).toBe('%');
    expect(unitForField(field('b', { unit: 'ratio' })).unit).toBe('×');
  });

  it('prefers an explicit step over the unit default', () => {
    expect(unitForField(field('layer_height', { unit: 'mm' })).step).toBe(0.1);
    expect(unitForField(field('layer_height', { unit: 'mm', step: 0.01 })).step).toBe(0.01);
  });

  it('never offers a fractional step for an integer field', () => {
    const f = field('raft_layers', { type: 'integer', unit: 'mm', step: 0.01 });
    expect(unitForField(f).step).toBe(1);
  });

  it('falls back to dimensionless when the schema says nothing', () => {
    expect(unitForField(field('mystery'))).toEqual({ unit: '', step: 1, scale: 1 });
  });
});

describe('fieldLabel', () => {
  it('uses the curated label when there is one', () => {
    expect(fieldLabel('layer_height')).toBe('Layer Height');
  });

  it('shows the raw key when there is not', () => {
    // Deliberately not humanised: a manufactured label looks authored, and a
    // reader cannot tell the two apart. The key is at least unambiguous.
    expect(fieldLabel('some_parameter_nobody_named_yet')).toBe('some_parameter_nobody_named_yet');
  });
});

describe('the engine schema', () => {
  const schema = processSchema as unknown as { $defs: Record<string, Record<string, unknown>> };
  const fields = parseSchema({ ...schema.$defs['SlicingParams'], $defs: schema.$defs }).fields;

  /**
   * Units are declared, never inferred. A numeric parameter without `x-unit`
   * renders as a bare number, which is how `travel_speed_mm_min` once showed a
   * multiplier sign.
   */
  it('declares a unit for every numeric parameter', () => {
    const missing = fields
      .filter((f) => f.type === 'number' || f.type === 'integer')
      .filter((f) => !f.unit)
      .map((f) => f.key);
    expect(missing, `numeric fields without x-unit: ${missing.join(', ')}`).toEqual([]);
  });

  it('only uses units the UI knows how to render', () => {
    const unknown = fields
      .filter((f) => f.unit && unitForField(f).unit === '' && f.unit !== 'count')
      .map((f) => `${f.key}=${f.unit}`);
    expect(unknown).toEqual([]);
  });

  /**
   * Labels are curated, never generated — so a parameter nobody has named shows
   * its raw schema key to the user. That is the right *behaviour* (see
   * `fieldLabel` above) and the wrong *outcome*: the reader should never meet
   * `extruder_clearance_radius_mm` in the sidebar. Failing here turns it into a
   * build-time defect, caught by whoever added the field.
   */
  it('has a curated label for every parameter', () => {
    const unnamed = fields.filter((f) => !(f.key in FIELD_LABELS)).map((f) => f.key);
    expect(unnamed, `parameters with no entry in FIELD_LABELS: ${unnamed.join(', ')}`).toEqual([]);
  });

  it('has a curated label for every enum choice', () => {
    const unnamed = fields
      .flatMap((f) => (f.enumOptions ?? []).map((o) => ({ key: f.key, value: o.value })))
      .filter(({ value }) => !(value in ENUM_LABELS))
      .map(({ key, value }) => `${key}.${value}`);
    expect(unnamed, `enum consts with no entry in ENUM_LABELS: ${unnamed.join(', ')}`).toEqual([]);
  });
});
