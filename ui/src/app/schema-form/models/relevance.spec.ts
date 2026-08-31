import { describe, expect, it } from 'vitest';
import globalSettingsSchema from '../../../schemas/slicer-engine-global-settings-v1.json';
import { isFieldRelevant } from './relevance';
import { parseSchema } from './schema-parser';
import type { FieldDef } from './field-def';

/**
 * A control the user can move but that changes nothing is worse than no control
 * at all, so every setting that only applies in some configurations carries an
 * `x-relevant-when` gate. Until now a gate could only test string/boolean
 * equality, which left numeric feature switches — a compensation that is off at
 * `0` — with no way to hide the settings that tune them.
 *
 * These tests pin the `greaterThan` operator against the real engine schema, so
 * the gate and the field it guards cannot drift apart.
 */
describe('field relevance gates', () => {
  const schema = globalSettingsSchema as unknown as {
    $defs: Record<string, Record<string, unknown>>;
  };
  const parsed = parseSchema({ ...schema.$defs['SlicingParams'], $defs: schema.$defs });

  function field(key: string): FieldDef {
    const found = parsed.fields.find((f) => f.key === key);
    expect(found, `${key} missing from the parsed schema`).toBeDefined();
    return found!;
  }

  it('parses a greaterThan gate off the engine schema', () => {
    expect(field('elephant_foot_layers').relevantWhen).toEqual({
      field: 'elephant_foot_compensation_mm',
      equals: undefined,
      greaterThan: 0,
    });
  });

  it('hides the elephant-foot tuning while the compensation is off', () => {
    const layers = field('elephant_foot_layers');
    const minWidth = field('elephant_foot_min_contour_width_mm');

    for (const off of [{ elephant_foot_compensation_mm: 0 }, {}]) {
      expect(isFieldRelevant(layers, off)).toBe(false);
      expect(isFieldRelevant(minWidth, off)).toBe(false);
    }

    const on = { elephant_foot_compensation_mm: 0.2 };
    expect(isFieldRelevant(layers, on)).toBe(true);
    expect(isFieldRelevant(minWidth, on)).toBe(true);
  });

  it('treats a non-numeric gate value as not greater than', () => {
    const layers = field('elephant_foot_layers');
    expect(isFieldRelevant(layers, { elephant_foot_compensation_mm: 'lots' })).toBe(false);
    expect(isFieldRelevant(layers, { elephant_foot_compensation_mm: null })).toBe(false);
  });

  it('still evaluates the equality gates it always did', () => {
    const airGap = field('raft_air_gap');
    expect(airGap.relevantWhen?.equals).toBe('raft');
    expect(isFieldRelevant(airGap, { adhesion_type: 'raft' })).toBe(true);
    expect(isFieldRelevant(airGap, { adhesion_type: 'brim' })).toBe(false);
  });

  it('leaves an ungated field always relevant', () => {
    expect(field('elephant_foot_compensation_mm').relevantWhen).toBeUndefined();
    expect(isFieldRelevant(field('elephant_foot_compensation_mm'), {})).toBe(true);
  });
});
