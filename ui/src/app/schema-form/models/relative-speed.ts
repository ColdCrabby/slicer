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

/** The toggle's two modes, in the order it cycles them. */
export const RELATIVE_SPEED_MODES: readonly UnitOption[] = [
  { id: 'mm_s', label: 'mm/s' },
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
