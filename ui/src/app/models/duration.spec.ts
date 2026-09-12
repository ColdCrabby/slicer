import { describe, expect, it } from 'vitest';
import { durationParts } from './duration';

/**
 * Only the arithmetic is asserted. `formatDuration` hands these fields to
 * `Intl.DurationFormat`, whose output is the runtime locale's business — so
 * pinning its punctuation here would test ICU and fail on a machine that is not
 * running in English.
 */
describe('durationParts', () => {
  it('picks a tier per magnitude', () => {
    expect(durationParts(940)).toEqual({ tier: 'milliseconds', fields: { milliseconds: 940 } });
    expect(durationParts(2519)).toEqual({
      tier: 'seconds',
      fields: { seconds: 2, milliseconds: 500 },
    });
    expect(durationParts(72500)).toEqual({ tier: 'minutes', fields: { minutes: 1, seconds: 13 } });
    expect(durationParts(4320000)).toEqual({ tier: 'hours', fields: { hours: 1, minutes: 12 } });
  });

  it('rounds before splitting, so no field can overflow the unit it is named for', () => {
    // Splitting first and rounding after gives seconds: 60 and milliseconds: 1000 here.
    expect(durationParts(119600)).toEqual({ tier: 'minutes', fields: { minutes: 2, seconds: 0 } });
    expect(durationParts(59960)).toEqual({ tier: 'minutes', fields: { minutes: 1, seconds: 0 } });
    expect(durationParts(999.6)).toEqual({
      tier: 'seconds',
      fields: { seconds: 1, milliseconds: 0 },
    });
    expect(durationParts(3599600)).toEqual({ tier: 'hours', fields: { hours: 1, minutes: 0 } });
  });

  it('keeps a zero field so the formatter can decide whether to show it', () => {
    expect(durationParts(0)).toEqual({ tier: 'milliseconds', fields: { milliseconds: 0 } });
    expect(durationParts(180000)).toEqual({ tier: 'minutes', fields: { minutes: 3, seconds: 0 } });
  });

  it('rejects a duration that cannot be measured', () => {
    expect(durationParts(-1)).toBeNull();
    expect(durationParts(NaN)).toBeNull();
    expect(durationParts(Infinity)).toBeNull();
  });
});
