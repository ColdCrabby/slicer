import type { UnitOption } from './field-units';

/**
 * The two shapes a `RelativeSpeed` value takes on the wire — a bare number is
 * absolute mm/s, a string ending in `%` is a fraction of the source field.
 * Mirrors `RelativeSpeed`'s own `Serialize`/`Deserialize` in
 * `src/settings/relative_speed.rs`.
 *
 * Shared between the two surfaces that render a `relative-speed` field —
 * the slice sidebar's `RelativeSpeedField` widget and the profile editor's
 * `ParamField` — the same way `field-units.ts`'s `toDisplay`/`toStored` are:
 * each surface draws its own control, but neither re-derives this parsing.
 */
export type ParsedRelativeSpeed =
  { kind: 'absolute'; mmS: number } | { kind: 'percent'; fraction: number };

/**
 * The three states the field's unit control cycles through. `mm_s`/`mm_min`
 * are both "absolute" — the *stored* value never changes between them, only
 * how it's read, exactly like any other speed field's unit toggle (and driven
 * by the same global `UnitPreference` "speed" family). `percent` is a
 * genuinely different stored shape, entered and left by converting against
 * the source field's current value.
 */
export const RELATIVE_SPEED_MODES: readonly UnitOption[] = [
  { id: 'mm_s', label: 'mm/s' },
  { id: 'mm_min', label: 'mm/min' },
  { id: 'percent', label: '%' },
];

export function parseRelativeSpeed(raw: unknown, fallback: unknown): ParsedRelativeSpeed {
  const value = raw === null || raw === undefined || raw === '' ? fallback : raw;
  if (typeof value === 'string') {
    const trimmed = value.trim();
    if (trimmed.endsWith('%')) {
      const n = Number(trimmed.slice(0, -1));
      return { kind: 'percent', fraction: (Number.isFinite(n) ? n : 0) / 100 };
    }
    const n = Number(trimmed);
    return { kind: 'absolute', mmS: Number.isFinite(n) ? n : 0 };
  }
  const n = Number(value ?? 0);
  return { kind: 'absolute', mmS: Number.isFinite(n) ? n : 0 };
}

/** Round to a couple of decimals — enough precision, no float noise. */
export function roundRelative(n: number): number {
  return Math.round(n * 100) / 100;
}

/** `mm/s` → `1`, `mm/min` → `60` — the same factor `field-units.ts` uses. */
export function relativeSpeedScale(modeId: string): number {
  return modeId === 'mm_min' ? 60 : 1;
}

/** Suffix shown inside the control for a given mode id. */
export function relativeSpeedUnit(modeId: string): string {
  if (modeId === 'percent') return '%';
  return modeId === 'mm_min' ? 'mm/min' : 'mm/s';
}

/** Stepper increment for a given mode id, in that mode's own units. */
export function relativeSpeedStep(modeId: string): number {
  return modeId === 'mm_min' ? 300 : 5;
}
