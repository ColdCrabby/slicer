import type { UnitOption } from './field-units';

/**
 * The two shapes a relative value takes on the wire — a bare number is
 * absolute, a string ending in `%` is a fraction of the field named by the
 * schema's `x-relative-to` or `x-derived-from` extension.
 *
 * Two engine mechanisms serialize this way and share this parsing:
 *
 * - `RelativeSpeed` (`src/settings/relative_speed.rs`), which carries the
 *   percentage all the way to the point of use, because the speed it is a
 *   fraction of is chosen per segment.
 * - the derived settings in `DERIVED_FROM` (`src/settings/params.rs`), resolved
 *   once against a sibling in the same document — a bead width as a percentage
 *   of the nozzle, so one process profile fits every machine the user owns.
 *
 * Shared between the two surfaces that render such a field — the slice
 * sidebar's `RelativeValueField` widget and the profile editor's `ParamField` —
 * the same way `field-units.ts`'s `toDisplay`/`toStored` are: each surface
 * draws its own control, but neither re-derives this parsing.
 */
export type ParsedRelativeValue =
  { kind: 'absolute'; value: number } | { kind: 'percent'; fraction: number };

/**
 * The modes a relative field's unit control cycles through, for a given
 * `x-unit` family.
 *
 * The absolute modes are alternative *readings* of one stored number, exactly
 * like any other field's unit toggle. `percent` is a genuinely different stored
 * shape, entered and left by converting against the source field's current
 * value — which is what stops the number on screen from silently meaning
 * something else after a click.
 *
 * A width has no second absolute reading, so it cycles two states rather than
 * three. Offering it `mm/min` would be offering a speed for a distance.
 */
export function relativeModes(unit: string | undefined): readonly UnitOption[] {
  if (unit === 'mm_s' || unit === 'mm_min') {
    return [
      { id: 'mm_s', label: 'mm/s' },
      { id: 'mm_min', label: 'mm/min' },
      { id: 'percent', label: '%' },
    ];
  }
  return [
    { id: 'absolute', label: unit === 'mm' ? 'mm' : '' },
    { id: 'percent', label: '%' },
  ];
}

export function parseRelativeValue(raw: unknown, fallback: unknown): ParsedRelativeValue {
  const value = raw === null || raw === undefined || raw === '' ? fallback : raw;
  if (typeof value === 'string') {
    const trimmed = value.trim();
    if (trimmed.endsWith('%')) {
      const n = Number(trimmed.slice(0, -1));
      return { kind: 'percent', fraction: (Number.isFinite(n) ? n : 0) / 100 };
    }
    const n = Number(trimmed);
    return { kind: 'absolute', value: Number.isFinite(n) ? n : 0 };
  }
  const n = Number(value ?? 0);
  return { kind: 'absolute', value: Number.isFinite(n) ? n : 0 };
}

/** Round to a couple of decimals — enough precision, no float noise. */
export function roundRelative(n: number): number {
  return Math.round(n * 100) / 100;
}

/** `mm/min` reads the stored number ×60; every other absolute mode is ×1. */
export function relativeScale(modeId: string): number {
  return modeId === 'mm_min' ? 60 : 1;
}

/** Suffix shown inside the control for a given mode id. */
export function relativeUnit(modeId: string, unit: string | undefined): string {
  if (modeId === 'percent') return '%';
  if (modeId === 'mm_min') return 'mm/min';
  if (modeId === 'mm_s') return 'mm/s';
  return unit === 'mm' ? 'mm' : '';
}

/** Stepper increment for a given mode id, in that mode's own units. */
export function relativeStep(modeId: string, unit: string | undefined): number {
  if (modeId === 'mm_min') return 300;
  if (modeId === 'percent') return 5;
  if (modeId === 'mm_s') return 5;
  return unit === 'mm' ? 0.01 : 1;
}
