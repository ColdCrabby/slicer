import { describe, expect, it } from 'vitest';
import { makePrinter, type PrinterProfile } from '../../models/printer.model';
import {
  correctedMaterials,
  correctionsFor,
  mergedCorrections,
  withCorrections,
  withoutCorrection,
} from './material-corrections';

/** A machine that already knows two things about two different materials. */
function corrected(): PrinterProfile {
  return makePrinter({
    name: 'Voron 2.4',
    material_overlays: {
      PLA: { max_volumetric_speed: 24, pressure_advance: 0.032 },
      ABS: { fan_speed: 0.15 },
    },
  } as Partial<PrinterProfile>);
}

describe('material corrections', () => {
  it('reads one material without inventing the others', () => {
    expect(correctionsFor(corrected(), 'PLA')).toEqual({
      max_volumetric_speed: 24,
      pressure_advance: 0.032,
    });
    expect(correctionsFor(corrected(), 'PETG')).toEqual({});
    expect(correctionsFor(makePrinter(), 'PLA')).toEqual({});
  });

  // The failure this module exists to prevent: correcting one material must
  // never cost what was already measured about another.
  it('leaves every other material alone when one is rewritten', () => {
    const next = withCorrections(corrected(), 'PLA', { max_volumetric_speed: 26 });
    expect(next['ABS']).toEqual({ fan_speed: 0.15 });
    expect(next['PLA']).toEqual({ max_volumetric_speed: 26 });
  });

  // An empty correction is not a value of zero — it is the material's own value
  // standing again, which is exactly what its absence means to the engine.
  it('drops a material left with nothing rather than keeping an empty entry', () => {
    const next = withCorrections(corrected(), 'ABS', {});
    expect('ABS' in next).toBe(false);
    expect(next['PLA']).toBeDefined();
  });

  it('merges a patch over what the material already had', () => {
    expect(mergedCorrections(corrected(), 'PLA', { pressure_advance: 0.04 })).toEqual({
      max_volumetric_speed: 24,
      pressure_advance: 0.04,
    });
  });

  it('removes one setting without touching its neighbours', () => {
    expect(withoutCorrection(corrected(), 'PLA', 'pressure_advance')).toEqual({
      max_volumetric_speed: 24,
    });
  });

  it('lists only the materials that actually carry a correction', () => {
    const printer = makePrinter({
      material_overlays: { PLA: { flow_ratio: 0.98 }, PETG: {} },
    } as Partial<PrinterProfile>);
    expect(correctedMaterials(printer)).toEqual(['PLA']);
    expect(correctedMaterials(makePrinter())).toEqual([]);
  });

  it('round-trips an edit through read, merge and write', () => {
    const printer = corrected();
    const next = withCorrections(
      printer,
      'PETG',
      mergedCorrections(printer, 'PETG', { max_volumetric_speed: 12 }),
    );
    expect(next).toEqual({
      PLA: { max_volumetric_speed: 24, pressure_advance: 0.032 },
      ABS: { fan_speed: 0.15 },
      PETG: { max_volumetric_speed: 12 },
    });
  });
});
