import type { FieldDef } from './field-def';

/**
 * The unit a field is measured in, and how far one step of its control moves.
 *
 * The dedicated printer and filament editors hand-write these per control —
 * `unit="mm"`, `[step]="0.05"` — and read far better for it. The schema-driven
 * sidebar had no equivalent: every number rendered with no unit and a flat
 * `step: 0.01`, so nudging a nozzle temperature moved it from 220 to 220.01 and
 * a speed in mm/s looked like a bare count.
 *
 * The engine's schema carries no unit metadata, so this derives it from the
 * naming conventions the parameters already follow. Ordered most specific
 * first: an explicit `_mm_s` suffix beats the `*_speed` name pattern, which in
 * turn beats the bare `_mm` fallback.
 */
export interface FieldUnit {
  /** Suffix shown inside the control. Empty for dimensionless counts. */
  unit: string;
  /** Increment for one press of the stepper, or one arrow key. */
  step: number;
  /**
   * Factor between the stored value and the displayed one.
   *
   * Only a `fraction` sets this: the engine stores `0.6` and the user should
   * see `60 %`. Absent everywhere else, where the two are the same number.
   */
  scale?: number;
}

/** Dimensionless — a count, a ratio the user reads as a plain number. */
const COUNT: FieldUnit = { unit: '', step: 1 };

/**
 * Suffix → unit. Checked before the name patterns below, because a suffix is an
 * explicit statement by whoever named the parameter.
 */
const BY_SUFFIX: ReadonlyArray<readonly [RegExp, FieldUnit]> = [
  [/_mm_s$/, { unit: 'mm/s', step: 5 }],
  [/_mm3_s$/, { unit: 'mm³/s', step: 1 }],
  [/_mm$/, { unit: 'mm', step: 0.1 }],
  [/_percent$/, { unit: '%', step: 1 }],
  [/_deg$/, { unit: '°', step: 5 }],
  [/_px$/, { unit: 'px', step: 16 }],
  [/_layers$/, COUNT],
  [/_count$/, COUNT],
  [/_ratio$/, { unit: '', step: 0.05 }],
  [/_s$/, { unit: 's', step: 1 }],
];

/**
 * Name pattern → unit, for the ~134 parameters that carry no unit suffix at
 * all. These follow the vocabulary the slicer has always used: anything named
 * `*_speed` is mm/s, anything named `*_temp` is °C, and the various words for
 * a distance are all millimetres.
 */
const BY_NAME: ReadonlyArray<readonly [RegExp, FieldUnit]> = [
  // Volumetric flow before plain speed: `max_volumetric_speed` is mm³/s, and
  // the `speed` in its name would otherwise claim it for mm/s.
  [/volumetric/, { unit: 'mm³/s', step: 1 }],
  [/_g_cm3$|_g_cm³$/, { unit: 'g/cm³', step: 0.01 }],
  // Before the speed rule: a fan "speed" is a percentage of full power, not a
  // rate in mm/s, and `fan_speed` matches both patterns.
  [/fan/, { unit: '%', step: 5 }],
  [/speed$|^speed_/, { unit: 'mm/s', step: 5 }],
  [/acceleration|_accel$/, { unit: 'mm/s²', step: 100 }],
  // Substring, not a suffix: the first-layer variants are named
  // `nozzle_temp_first_layer`, which a `temp$` anchor misses entirely.
  [/temp|temperature/, { unit: '°C', step: 5 }],
  [/angle$/, { unit: '°', step: 5 }],
  // `density` is deliberately absent — `filament_density_g_cm3` is a material
  // property in g/cm³, and infill density has a slider of its own.
  [/overlap|_flow$|infill_anchor|_percent/, { unit: '%', step: 1 }],
  [
    /(height|width|length|distance|thickness|diameter|offset|gap|margin|radius|extrusion)(_min|_max)?$|brim|skirt_distance|_z$/,
    { unit: 'mm', step: 0.1 },
  ],
  [/time$|duration$/, { unit: 's', step: 1 }],
  [/layers$|loops$|count$|_every$/, COUNT],
];

/**
 * A step that is sane for the *magnitude* the field works at.
 *
 * A layer height lives near 0.2 and needs 0.01; a bed size lives near 220 and
 * would take a lifetime to reach at that increment. Applied only to millimetre
 * fields, where the range genuinely spans three orders of magnitude.
 *
 * The value currently in the box is the best reference available: a good many
 * dimensions default to `0` meaning "derive this from the nozzle", so the
 * default is precisely the case that says nothing about the working scale.
 */
function scaleMillimetreStep(step: number, field: FieldDef, current?: number): number {
  const candidates = [current, field.default, field.maximum];
  const reference = Math.abs(
    Number(candidates.find((c) => c !== undefined && c !== null && Number(c) !== 0) ?? 0),
  );
  if (!Number.isFinite(reference) || reference === 0) {
    return step;
  }
  if (reference < 1) {
    return 0.01;
  }
  if (reference < 10) {
    return 0.05;
  }
  return 1;
}

/**
 * Resolve the unit and step for a schema field.
 *
 * Integers never get a fractional step whatever else matches — a field the
 * engine types as `u32` cannot hold 2.5 walls, and offering it is a control
 * that lies about what it accepts.
 */
export function unitForField(field: FieldDef, current?: number): FieldUnit {
  const key = field.key;

  // An explicit `x-unit` from the schema outranks every guess below — it is the
  // engine stating what the number means rather than the UI inferring it from a
  // name, and it is the only way to tell a 0–1 fraction from a 0–100 percentage
  // when both are called `..._percent`.
  switch (field.unit) {
    case 'fraction':
      return { unit: '%', step: 5, scale: 100 };
    case 'percent':
      return { unit: '%', step: 5 };
    case 'ratio':
      return { unit: '×', step: 0.05 };
  }

  let resolved: FieldUnit | undefined;
  for (const [pattern, unit] of BY_SUFFIX) {
    if (pattern.test(key)) {
      resolved = unit;
      break;
    }
  }
  if (!resolved) {
    for (const [pattern, unit] of BY_NAME) {
      if (pattern.test(key)) {
        resolved = unit;
        break;
      }
    }
  }
  resolved ??= field.type === 'integer' ? COUNT : { unit: '', step: 0.1 };

  if (resolved.unit === 'mm') {
    resolved = { ...resolved, step: scaleMillimetreStep(resolved.step, field, current) };
  }
  if (field.type === 'integer') {
    resolved = { ...resolved, step: Math.max(1, Math.round(resolved.step)) };
  }
  return resolved;
}
