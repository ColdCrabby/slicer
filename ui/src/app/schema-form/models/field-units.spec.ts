import { describe, expect, it } from 'vitest';
import processSchema from '../../../schemas/slicer-engine-process-profile-v1.json';
import type { FieldDef } from './field-def';
import { ENUM_LABELS, FIELD_LABELS, fieldLabel } from './field-labels';
import { unitForField } from './field-units';
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
    expect(unitForField(field('mystery'))).toEqual({ unit: '', step: 1 });
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

/** Longest an option summary may be and still read as one line of help. */
const SUMMARY_MAX_CHARS = 140;

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

  /**
   * An option's summary is its doc comment's first paragraph, shown verbatim.
   * Nothing trims it at render time any more, so the doc has to be written
   * short — the blank line after the summary is where the detail goes.
   */
  it('keeps every enum summary short enough to show whole', () => {
    const long = fields
      .flatMap((f) => f.enumOptions ?? [])
      .filter((o) => (o.description?.length ?? 0) > SUMMARY_MAX_CHARS)
      .map((o) => `${o.value} (${o.description?.length})`);
    expect(long, `enum summaries over ${SUMMARY_MAX_CHARS} chars: ${long.join(', ')}`).toEqual([]);
  });

  it('leaves no Markdown in the text it renders', () => {
    // Stripping happens once, in the parser. A marker surviving into a
    // `FieldDef` means something bypassed it.
    const marked = [
      ...fields.map((f) => f.description),
      ...fields.flatMap((f) => (f.enumOptions ?? []).map((o) => o.description)),
    ].filter((text) => text && /\*\*|`/.test(text));
    expect(marked).toEqual([]);
  });

  it('has a curated label for every enum choice', () => {
    const unnamed = fields
      .flatMap((f) => (f.enumOptions ?? []).map((o) => ({ key: f.key, value: o.value })))
      .filter(({ value }) => !(value in ENUM_LABELS))
      .map(({ key, value }) => `${key}.${value}`);
    expect(unnamed, `enum consts with no entry in ENUM_LABELS: ${unnamed.join(', ')}`).toEqual([]);
  });
});
