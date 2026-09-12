import type { FieldDef } from './field-def';

/**
 * The unit a field is measured in, and how far one step of its control moves.
 *
 * Both come from the schema. `x-unit` is declared on every numeric parameter
 * and `x-step` on the ones whose per-unit default is wrong for their working
 * range — so this module is a lookup, not an inference.
 *
 * It used to guess from the parameter's name with two tables of regexes, which
 * was wrong often enough to matter and silently: `travel_speed_mm_min` and
 * `retract_speed_mm_min` fell through every pattern and rendered as a bare
 * multiplier, `filament_density_g_cm3` read as a percentage because its name
 * contains "density", and `max_volumetric_speed` had to be special-cased out of
 * the mm/s rule. A name is not a unit.
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

/**
 * `x-unit` → what the control shows and how far it steps.
 *
 * The three that are not physical units are the ones a name could never have
 * told us apart:
 *
 * - `fraction` — stored 0–1, shown 0–100 %, converted in and out.
 * - `percent` — already on a 0–100 scale, and free to exceed it.
 * - `ratio` — a multiplier against something else (nozzle diameter, nominal
 *   flow), shown with `×` because it is not a proportion of a whole.
 */
const UNITS: Record<string, FieldUnit> = {
  mm: { unit: 'mm', step: 0.1 },
  mm2: { unit: 'mm²', step: 0.1 },
  mm_s: { unit: 'mm/s', step: 5 },
  mm_min: { unit: 'mm/min', step: 300 },
  mm_s2: { unit: 'mm/s²', step: 100 },
  mm3_s: { unit: 'mm³/s', step: 1 },
  deg: { unit: '°', step: 5 },
  celsius: { unit: '°C', step: 5 },
  s: { unit: 's', step: 1 },
  px: { unit: 'px', step: 16 },
  g_cm3: { unit: 'g/cm³', step: 0.01 },
  per_kg: { unit: '/kg', step: 1 },
  count: { unit: '', step: 1 },
  percent: { unit: '%', step: 5 },
  fraction: { unit: '%', step: 5, scale: 100 },
  ratio: { unit: '×', step: 0.05 },
};

/** Fallback for a field the schema has not annotated. */
const UNITLESS: FieldUnit = { unit: '', step: 1 };

/**
 * Resolve the unit and step for a schema field.
 *
 * An integer never gets a fractional step whatever the unit says: a field the
 * engine types as `u32` cannot hold 2.5 walls, and offering it is a control
 * that lies about what it accepts.
 */
export function unitForField(field: FieldDef): FieldUnit {
  const resolved: FieldUnit = (field.unit ? UNITS[field.unit] : undefined) ?? UNITLESS;
  const step = field.step ?? resolved.step;
  if (field.type === 'integer') {
    return { ...resolved, step: Math.max(1, Math.round(step)) };
  }
  return step === resolved.step ? resolved : { ...resolved, step };
}
