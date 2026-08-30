import { describe, expect, it } from 'vitest';
import globalSettingsSchema from '../../../schemas/slicer-engine-global-settings-v1.json';
import { parseSchema } from './schema-parser';
import type { FieldDef } from './field-def';

/**
 * Every infill pattern the engine can slice has to be reachable from the UI.
 *
 * This used to be a hand-written picker listing five patterns as a segmented
 * control. When the engine grew to ten, the extra five became unselectable —
 * silently, because nothing tied the widget to the schema. These tests pin the
 * property that matters: the control is driven by the schema, so a pattern
 * added to the engine shows up without anyone remembering to edit the UI.
 */
describe('infill pattern field', () => {
  // Same extraction the settings panel performs: the slicer settings live in
  // the `SlicingParams` definition, with `$defs` carried along for `$ref`s.
  const schema = globalSettingsSchema as unknown as {
    $defs: Record<string, Record<string, unknown>>;
  };
  const parsed = parseSchema({ ...schema.$defs['SlicingParams'], $defs: schema.$defs });

  function field(key: string): FieldDef {
    const found = parsed.fields.find((f) => f.key === key);
    expect(found, `${key} missing from the parsed schema`).toBeDefined();
    return found!;
  }

  it('offers every pattern the engine declares', () => {
    const options = field('infill_pattern').enumOptions ?? [];
    expect(options.map((o) => o.value)).toEqual([
      'Rectilinear',
      'AlignedRectilinear',
      'Grid',
      'Triangles',
      'TriHexagon',
      'Cubic',
      'Honeycomb',
      'Concentric',
      'Gyroid',
      'TpmsD',
    ]);
  });

  it('has enough options that the generic rule renders a dropdown', () => {
    // The registry sends any enum with more than RADIO_MAX_OPTIONS (3) choices
    // to `EnumSelect`. Asserting the count keeps that path in force without
    // importing Angular components into a plain unit test; a custom widget
    // override is what hid patterns in the first place.
    expect((field('infill_pattern').enumOptions ?? []).length).toBeGreaterThan(3);
  });

  it('gives every pattern a description to choose by', () => {
    for (const option of field('infill_pattern').enumOptions ?? []) {
      expect(option.description, `${option.value} has no description`).toBeTruthy();
    }
  });

  it('labels every pattern readably', () => {
    const labels = Object.fromEntries(
      (field('infill_pattern').enumOptions ?? []).map((o) => [o.value, o.label]),
    );
    expect(labels['AlignedRectilinear']).toBe('Aligned Rectilinear');
    expect(labels['TriHexagon']).toBe('Tri-Hexagon');
    // Not "Tpms D", which is what the generic humaniser produces.
    expect(labels['TpmsD']).toBe('TPMS-D');
  });

  it('offers the surface patterns as described dropdowns too', () => {
    for (const key of [
      'top_surface_pattern',
      'bottom_surface_pattern',
      'internal_solid_infill_pattern',
    ]) {
      const options = field(key).enumOptions ?? [];
      expect(
        options.map((o) => o.value),
        key,
      ).toEqual([
        'rectilinear',
        'aligned-rectilinear',
        'monotonic',
        'monotonic-line',
        'concentric',
      ]);
      expect(options.length, key).toBeGreaterThan(3);
      // Hyphenated values must read as words, not "Monotonic-line".
      expect(options.find((o) => o.value === 'monotonic-line')?.label).toBe('Monotonic Line');
    }
  });
});
