import { describe, expect, it } from 'vitest';
import globalSettingsSchema from '../../schemas/slicer-engine-global-settings-v1.json';
import { ENGINE_DEFAULTS, applyOverridePatch } from './slice-settings.model';

/**
 * The override diff is what the engine actually receives, so the floor it is
 * measured against has to be the engine's own defaults. A key missing from that
 * floor reads as "the user set this" and is sent on every slice — which is how
 * a hand-written default list silently pins settings nobody ever touched.
 */
describe('engine defaults', () => {
  const properties = (
    globalSettingsSchema.$defs.SlicingParams as { properties: Record<string, unknown> }
  ).properties;

  it('carries every default the schema declares', () => {
    const declared = Object.entries(properties)
      .filter(([, prop]) => (prop as { default?: unknown }).default !== undefined)
      .map(([key]) => key);
    expect(declared.length).toBeGreaterThan(100);
    const missing = declared.filter((key) => !(key in ENGINE_DEFAULTS));
    expect(missing, `defaults absent from ENGINE_DEFAULTS: ${missing.join(', ')}`).toEqual([]);
  });

  it('invents nothing the schema does not declare', () => {
    const strays = Object.keys(ENGINE_DEFAULTS).filter((key) => !(key in properties));
    expect(strays, `keys with no schema property: ${strays.join(', ')}`).toEqual([]);
  });

  it('takes each value verbatim from the schema', () => {
    for (const [key, value] of Object.entries(ENGINE_DEFAULTS)) {
      expect(value, key).toEqual((properties[key] as { default?: unknown }).default);
    }
  });
});

describe('override diff', () => {
  const baseline = { layer_height: 0.2, nozzle_temp: 210, infill_pattern: 'Gyroid' };

  it('records a value that deviates from the preset stack', () => {
    expect(applyOverridePatch({}, { layer_height: 0.12 }, baseline)).toEqual({
      layer_height: 0.12,
    });
  });

  it('inherits again when a value is set back to the preset', () => {
    const pinned = { layer_height: 0.12 };
    expect(applyOverridePatch(pinned, { layer_height: 0.2 }, baseline)).toEqual({});
  });

  it('leaves the settings it was not asked about alone', () => {
    const pinned = { nozzle_temp: 230 };
    expect(applyOverridePatch(pinned, { layer_height: 0.12 }, baseline)).toEqual({
      nozzle_temp: 230,
      layer_height: 0.12,
    });
  });

  it('does not mutate the diff it was given', () => {
    const pinned = { layer_height: 0.12 };
    applyOverridePatch(pinned, { layer_height: 0.2 }, baseline);
    expect(pinned).toEqual({ layer_height: 0.12 });
  });

  it('records a key the preset stack is silent about', () => {
    expect(applyOverridePatch({}, { bridge_angle: 45 }, baseline)).toEqual({ bridge_angle: 45 });
  });

  it('compares arrays and objects structurally, not by identity', () => {
    const triggers = [{ position_type: 'at_layer', layer: 5, action: 'pause' }];
    expect(
      applyOverridePatch({ triggers }, { triggers: structuredClone(triggers) }, { triggers }),
    ).toEqual({});
  });

  it('treats an absent baseline value and an explicit null as the same', () => {
    expect(applyOverridePatch({}, { start_gcode: null }, {})).toEqual({});
  });
});
