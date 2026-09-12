/**
 * Duration display, built on `Intl` rather than on string templates.
 *
 * `Intl.DurationFormat` owns the part that is genuinely hard: which unit names
 * to use, where the decimal separator goes, and how two fields are joined. A
 * German reader gets `1 Min., 12 Sek.` without this file knowing anything about
 * German.
 *
 * What is left is the arithmetic the API cannot do for us — it takes a duration
 * *record*, not a millisecond count — plus the one editorial decision, which is
 * how much precision each magnitude deserves. That part is {@link durationParts},
 * split out because it is the only half worth testing: the rounding has a
 * failure mode (a field overflowing its own unit) and the formatting is
 * somebody else's problem.
 *
 * `Intl.DurationFormat` needs Safari 16.4 and the Apple shells declare a floor
 * of 16.0, so that narrow window falls back to `Intl.NumberFormat`'s unit style
 * (ES2020, everywhere). It renders one unit rather than composing two — `1m
 * 12s` reads `1.2m` there — which is a worse answer, not a broken one.
 */

const MS_PER_SECOND = 1000;
const MS_PER_MINUTE = 60 * MS_PER_SECOND;
const MS_PER_HOUR = 60 * MS_PER_MINUTE;

/** The largest unit a duration is worth stating in. Picks the formatter. */
export type DurationTier = 'milliseconds' | 'seconds' | 'minutes' | 'hours';

/** A duration reduced to the fields worth showing, ready for `Intl`. */
export interface DurationParts {
  tier: DurationTier;
  fields: Partial<Record<'hours' | 'minutes' | 'seconds' | 'milliseconds', number>>;
}

/**
 * Reduce a millisecond count to the fields worth showing.
 *
 * Rounds *before* splitting, at the precision the chosen tier will actually
 * show. Splitting first and rounding after is what produces `1m 60s` from
 * 119.6 s, and `60.0s` from 59.96 s — the field overflows the unit it is
 * named for. Returns `null` for a duration that cannot be measured.
 */
export function durationParts(ms: number): DurationParts | null {
  if (!Number.isFinite(ms) || ms < 0) {
    return null;
  }

  const wholeMs = Math.round(ms);
  if (wholeMs < MS_PER_SECOND) {
    return { tier: 'milliseconds', fields: { milliseconds: wholeMs } };
  }

  // Tenths of a second — the finest thing any tier below this shows.
  const tenths = Math.round(ms / 100);
  if (tenths < 600) {
    return {
      tier: 'seconds',
      fields: { seconds: Math.floor(tenths / 10), milliseconds: (tenths % 10) * 100 },
    };
  }

  const totalSeconds = Math.round(ms / MS_PER_SECOND);
  if (totalSeconds < 3600) {
    return {
      tier: 'minutes',
      fields: { minutes: Math.floor(totalSeconds / 60), seconds: totalSeconds % 60 },
    };
  }

  const totalMinutes = Math.round(ms / MS_PER_MINUTE);
  return {
    tier: 'hours',
    fields: { hours: Math.floor(totalMinutes / 60), minutes: totalMinutes % 60 },
  };
}

/** `undefined` locale = the viewer's own, which on desktop is the OS's. */
function durationFormat(options: Intl.DurationFormatOptions): Intl.DurationFormat | null {
  try {
    return new Intl.DurationFormat(undefined, { style: 'narrow', ...options });
  } catch {
    return null;
  }
}

const narrow = durationFormat({});

/**
 * A zero field is dropped by default — right for `3m` (rather than `3m 0s`) and
 * wrong for the one tier where milliseconds are the only field, because there
 * it renders nothing at all.
 */
const milliseconds = durationFormat({ millisecondsDisplay: 'always' });

/**
 * Renders sub-second detail as a fraction of the seconds field (`2.5s`) instead
 * of a field of its own (`2s 519ms`), which is what a bare narrow style gives.
 */
const fractionalSeconds = durationFormat({ milliseconds: 'numeric', fractionalDigits: 1 });

const FORMATTERS: Record<DurationTier, Intl.DurationFormat | null> = {
  milliseconds,
  seconds: fractionalSeconds,
  minutes: narrow,
  hours: narrow,
};

/** One unit, no composition — the pre-16.4 WebKit path. */
function formatUnit(ms: number, tier: DurationTier): string {
  const [unit, divisor, maximumFractionDigits] = {
    milliseconds: ['millisecond', 1, 0],
    seconds: ['second', MS_PER_SECOND, 1],
    minutes: ['minute', MS_PER_MINUTE, 1],
    hours: ['hour', MS_PER_HOUR, 1],
  }[tier] as ['millisecond' | 'second' | 'minute' | 'hour', number, 0 | 1];

  return new Intl.NumberFormat(undefined, {
    style: 'unit',
    unit,
    unitDisplay: 'narrow',
    maximumFractionDigits,
  }).format(ms / divisor);
}

/**
 * Format a millisecond duration for display: `940ms`, `2.5s`, `1m 12s`,
 * `1h 12m`. Negative and non-finite inputs render as an empty string.
 */
export function formatDuration(ms: number): string {
  const parts = durationParts(ms);
  if (!parts) {
    return '';
  }
  return FORMATTERS[parts.tier]?.format(parts.fields) ?? formatUnit(ms, parts.tier);
}
