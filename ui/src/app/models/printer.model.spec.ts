import { describe, expect, it } from 'vitest';
import { ENUM_LABELS } from '../schema-form/models/field-labels';
import { PRINTER_GCODE_FLAVORS, asGcodeFlavor } from './printer.model';

/**
 * The firmware picker is read from the engine's `GcodeFlavor`, so a dialect the
 * engine adds is offered without a UI change — but only its curated label
 * keeps it from reading as a raw token like `reprapfirmware`.
 */
describe('printer firmware flavors', () => {
  it('offers every engine flavor, RepRapFirmware included', () => {
    expect(PRINTER_GCODE_FLAVORS.map((f) => f.value)).toEqual([
      'marlin',
      'klipper',
      'reprapfirmware',
    ]);
  });

  it('labels every flavor rather than showing its token', () => {
    const unlabelled = PRINTER_GCODE_FLAVORS.filter((f) => !ENUM_LABELS[f.value]);
    expect(unlabelled.map((f) => f.value)).toEqual([]);
    expect(PRINTER_GCODE_FLAVORS.find((f) => f.value === 'reprapfirmware')?.label).toBe(
      'RepRapFirmware',
    );
  });

  it('accepts a detected firmware only when the engine has a dialect for it', () => {
    expect(asGcodeFlavor(' Klipper ')).toBe('klipper');
    expect(asGcodeFlavor('reprapfirmware')).toBe('reprapfirmware');
    expect(asGcodeFlavor('smoothie')).toBeUndefined();
    expect(asGcodeFlavor(undefined)).toBeUndefined();
  });
});
