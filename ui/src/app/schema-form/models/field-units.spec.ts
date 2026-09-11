import { describe, expect, it } from 'vitest';
import type { FieldDef } from './field-def';
import { fieldLabel } from './field-labels';
import { unitForField } from './field-units';

function field(key: string, extra: Partial<FieldDef> = {}): FieldDef {
  return { key, type: 'number', required: false, ...extra };
}

describe('unitForField', () => {
  it('reads a unit suffix in preference to a name pattern', () => {
    // `_mm_s` is explicit; the bare `speed` pattern would also match.
    expect(unitForField(field('travel_speed_mm_s')).unit).toBe('mm/s');
  });

  it('scales a millimetre step to the magnitude the field works at', () => {
    expect(unitForField(field('layer_height', { default: 0.2 })).step).toBe(0.01);
    expect(unitForField(field('brim_width', { default: 5 })).step).toBe(0.05);
    expect(unitForField(field('skirt_distance', { default: 200 })).step).toBe(1);
  });

  it('never offers a fractional step for an integer field', () => {
    const f = field('fuzzy_skin_thickness_mm', { type: 'integer', default: 0.3 });
    expect(unitForField(f).step).toBeGreaterThanOrEqual(1);
    expect(Number.isInteger(unitForField(f).step)).toBe(true);
  });

  it('catches the first-layer temperature variants, not just the bare ones', () => {
    expect(unitForField(field('nozzle_temp')).unit).toBe('°C');
    expect(unitForField(field('nozzle_temp_first_layer')).unit).toBe('°C');
    expect(unitForField(field('chamber_temp_first_layer')).unit).toBe('°C');
  });

  it('honours an explicit x-unit over every name-based guess', () => {
    // Stored 0–1, shown 0–100: `fan_speed: 1.0` is full speed, not one percent.
    expect(unitForField(field('fan_speed', { unit: 'fraction' }))).toEqual({
      unit: '%',
      step: 5,
      scale: 100,
    });
    // Already on a 0–100 scale, and free to exceed it.
    expect(unitForField(field('infill_anchor_percent', { unit: 'percent' }))).toEqual({
      unit: '%',
      step: 5,
    });
    // A multiple of something else, so not a proportion of a whole.
    expect(unitForField(field('wall_line_width_max', { unit: 'ratio' }))).toEqual({
      unit: '×',
      step: 0.05,
    });
  });

  it('reads a fan speed as a percentage, not a rate', () => {
    // `fan_speed` matches both the fan and the speed pattern; fan has to win.
    expect(unitForField(field('fan_speed')).unit).toBe('%');
    expect(unitForField(field('bridge_fan_speed')).unit).toBe('%');
    expect(unitForField(field('first_layer_fan_speed')).unit).toBe('%');
    expect(unitForField(field('print_speed')).unit).toBe('mm/s');
  });

  it('drops a unit suffix from the label, since the control shows the unit', () => {
    expect(fieldLabel('filament_density_g_cm3')).toBe('Filament Density');
    expect(fieldLabel('min_layer_time_s')).toBe('Min Layer Time');
  });

  it('does not mistake volumetric flow for a linear speed', () => {
    expect(unitForField(field('max_volumetric_speed')).unit).toBe('mm³/s');
    expect(unitForField(field('print_speed')).unit).toBe('mm/s');
  });

  it('treats filament density as a material property, not a percentage', () => {
    expect(unitForField(field('filament_density_g_cm3')).unit).toBe('g/cm³');
  });

  it('leaves a plain count dimensionless and stepping by one', () => {
    expect(unitForField(field('wall_count', { type: 'integer' }))).toEqual({ unit: '', step: 1 });
    expect(unitForField(field('top_layers', { type: 'integer' })).unit).toBe('');
  });

  it('covers the min/max variants of a dimension', () => {
    expect(unitForField(field('wall_line_width_min')).unit).toBe('mm');
    expect(unitForField(field('wall_line_width_max')).unit).toBe('mm');
  });
});
