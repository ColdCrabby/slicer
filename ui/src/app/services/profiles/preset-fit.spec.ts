import { describe, expect, it } from 'vitest';
import { makePrinter, type PrinterProfile } from '../../models/printer.model';
import { makeFilament } from '../../models/filament.model';
import { filamentFitWarning, processFitWarning } from './preset-fit';

/** The old Marlin bedslinger: modest limits, PTFE hotend, no enclosure. */
function oldMachine(overrides: Record<string, unknown> = {}): PrinterProfile {
  return makePrinter({
    name: 'Ender 3',
    params: {
      max_velocity: 150,
      acceleration: 3000,
      max_hotend_temp: 240,
      max_bed_temp: 80,
      heated_chamber: false,
      ...overrides,
    },
  });
}

describe('processFitWarning', () => {
  it('says so when a preset asks for more speed than the machine tops out at', () => {
    const fast = { id: 'p', name: 'Maximum', params: { print_speed: 300 } } as never;
    expect(processFitWarning(fast, oldMachine())).toMatch(/300 mm\/s.*Ender 3.*150/);
  });

  it('falls through to acceleration when the speed is within reach', () => {
    const fast = {
      id: 'p',
      name: 'Maximum',
      params: { print_speed: 120, acceleration: 25000 },
    } as never;
    expect(processFitWarning(fast, oldMachine())).toMatch(/25000 mm\/s².*3000/);
  });

  it('stays quiet for a preset the machine can carry', () => {
    const standard = {
      id: 'p',
      name: 'Standard',
      params: { print_speed: 120, acceleration: 3000 },
    } as never;
    expect(processFitWarning(standard, oldMachine())).toBeNull();
  });

  // A machine nobody has described states no limits, and an unstated limit must
  // never read as a limit of zero — every preset would look like a stretch.
  it('stays quiet when the machine has declared no limits', () => {
    const fast = { id: 'p', name: 'Maximum', params: { print_speed: 300 } } as never;
    const undescribed = makePrinter({ name: 'Custom', params: {} });
    expect(processFitWarning(fast, undescribed)).toBeNull();
  });
});

describe('filamentFitWarning', () => {
  it('catches a material hotter than the hotend is rated for', () => {
    const abs = makeFilament({ material: 'ABS' });
    expect(filamentFitWarning(abs, oldMachine())).toMatch(/255 °C.*240/);
  });

  it('catches a bed target the bed cannot reach', () => {
    const abs = makeFilament({ material: 'ABS' });
    // An all-metal hotend clears the first check, so the bed is what is left.
    expect(filamentFitWarning(abs, oldMachine({ max_hotend_temp: 300 }))).toMatch(/105 °C bed.*80/);
  });

  it('catches a material that wants a chamber the machine has no heater for', () => {
    const abs = makeFilament({ material: 'ABS' });
    const openFrame = oldMachine({ max_hotend_temp: 300, max_bed_temp: 120 });
    expect(filamentFitWarning(abs, openFrame)).toMatch(/chamber.*Ender 3.*no chamber heater/);
  });

  it('stays quiet for a material the machine is built for', () => {
    const abs = makeFilament({ material: 'ABS' });
    const enclosed = oldMachine({
      max_hotend_temp: 300,
      max_bed_temp: 120,
      heated_chamber: true,
    });
    expect(filamentFitWarning(abs, enclosed)).toBeNull();
  });

  it('stays quiet for PLA on the modest machine', () => {
    expect(filamentFitWarning(makeFilament({ material: 'PLA' }), oldMachine())).toBeNull();
  });
});
