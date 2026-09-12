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
 *
 * **What the engine stores and what the user reads are two different units.**
 * The engine keeps travel and retraction speed in mm/min because that is what
 * goes on the `F` word of a G-code move, and it keeps proportions as a fraction
 * because that is what the arithmetic wants. Neither is what a 3D-printing user
 * thinks in. A {@link UnitFamily} holds the conversions, so a field declares
 * the unit it is *stored* in and the form decides the unit it is *shown* in.
 */
export interface FieldUnit {
  /** Suffix shown inside the control. Empty for dimensionless counts. */
  unit: string;
  /** Increment for one press of the stepper, or one arrow key — in display units. */
  step: number;
  /**
   * Factor between the stored value and the displayed one.
   *
   * `1` wherever the two are the same number, which is most fields. A stored
   * fraction of `0.6` shown as `60 %` has a scale of 100; a stored 9000 mm/min
   * shown as `150 mm/s` has a scale of 1/60.
   */
  scale: number;
  /**
   * The family whose display unit the user may switch, when there is more than
   * one sensible answer. Absent for a field with nothing to switch to.
   */
  family?: string;
  /** Every display unit the family offers, in the order the toggle cycles them. */
  options?: readonly UnitOption[];
}

/** One display unit a user may pick for a family. */
export interface UnitOption {
  /** `x-unit` id, and what the preference stores. */
  id: string;
  /** Suffix shown inside the control. */
  label: string;
}

/**
 * A unit and its size relative to the base unit of its family.
 *
 * `factor` is how many of this unit make one base unit: one mm/s is 60 mm/min,
 * so `mm_min` has a factor of 60. A field stored in unit *s* and shown in unit
 * *d* therefore scales by `d.factor / s.factor`, which is exact for the pairs
 * that matter and rounded at the edges by {@link toDisplay} / {@link toStored}.
 */
interface UnitDef {
  unit: string;
  step: number;
  family?: string;
  factor?: number;
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
const UNITS: Record<string, UnitDef> = {
  mm: { unit: 'mm', step: 0.1 },
  mm2: { unit: 'mm²', step: 0.1 },
  mm_s: { unit: 'mm/s', step: 5, family: 'speed', factor: 1 },
  mm_min: { unit: 'mm/min', step: 300, family: 'speed', factor: 60 },
  mm_s2: { unit: 'mm/s²', step: 100 },
  mm3_s: { unit: 'mm³/s', step: 1 },
  deg: { unit: '°', step: 5 },
  celsius: { unit: '°C', step: 5 },
  s: { unit: 's', step: 1 },
  px: { unit: 'px', step: 16 },
  g_cm3: { unit: 'g/cm³', step: 0.01 },
  per_kg: { unit: '/kg', step: 1 },
  count: { unit: '', step: 1 },
  percent: { unit: '%', step: 5, family: 'proportion', factor: 100 },
  fraction: { unit: '%', step: 5, family: 'proportion', factor: 1 },
  ratio: { unit: '×', step: 0.05 },
};

/**
 * Families of interchangeable units: which one a field is shown in by default,
 * and which ones the user may switch between.
 *
 * `offers` is deliberately not "every member of the family". A proportion is
 * stored as a fraction and read as a percentage, and nobody wants to be offered
 * `0.6` — the family exists there only to carry the ×100, which is what the old
 * `scale` field did by hand. Speed is the one with a real choice in it: the
 * engine stores travel and retraction in mm/min, everything else in mm/s, and
 * the user reads all of it in whichever they think in.
 *
 * Adding a family is this table plus a `factor` on the units it spans.
 */
interface UnitFamily {
  /** Shown when the user has expressed no preference. */
  fallback: string;
  /** Display units the user may cycle through; one entry means no toggle. */
  offers: readonly string[];
}

const FAMILIES: Record<string, UnitFamily> = {
  // mm/s is what a 3D-printing user thinks in, whatever the G-code carries.
  speed: { fallback: 'mm_s', offers: ['mm_s', 'mm_min'] },
  proportion: { fallback: 'percent', offers: ['percent'] },
};

/** Fallback for a field the schema has not annotated. */
const UNITLESS: FieldUnit = { unit: '', step: 1, scale: 1 };

/** The display unit the user has chosen, per family. */
export type UnitDisplay = Readonly<Record<string, string>>;

/** Every family with a real choice in it, for the settings that offer one. */
export function switchableFamilies(): readonly string[] {
  return Object.keys(FAMILIES).filter((id) => FAMILIES[id].offers.length > 1);
}

/** The units a family offers, labelled — in the order a toggle cycles them. */
export function familyOptions(family: string): readonly UnitOption[] {
  return (FAMILIES[family]?.offers ?? []).map((id) => ({ id, label: UNITS[id].unit }));
}

/** Whether `id` is a display unit the named family actually offers. */
export function offersUnit(family: string, id: string | null | undefined): boolean {
  return !!id && (FAMILIES[family]?.offers ?? []).includes(id);
}

/**
 * Resolve the unit, step and conversion for a schema field.
 *
 * `display` is the user's chosen unit per family; an absent or unknown choice
 * falls back to the family's own default, so a corrupt preference degrades to
 * the calm answer rather than to a bare number.
 *
 * An integer never gets a fractional step whatever the unit says: a field the
 * engine types as `u32` cannot hold 2.5 walls, and offering it is a control
 * that lies about what it accepts. That rounding only applies while stored and
 * displayed are the same number — a converted integer is not one.
 */
export function unitForField(field: FieldDef, display: UnitDisplay = {}): FieldUnit {
  const stored = field.unit ? UNITS[field.unit] : undefined;
  if (!stored) {
    const raw = field.step ?? UNITLESS.step;
    const step = field.type === 'integer' ? Math.max(1, Math.round(raw)) : raw;
    return step === UNITLESS.step ? UNITLESS : { ...UNITLESS, step };
  }

  const family = stored.family;
  const shownId = family ? displayUnitOf(family, display) : field.unit!;
  const shown = UNITS[shownId];
  const scale = family ? (shown.factor ?? 1) / (stored.factor ?? 1) : 1;

  // An explicit `x-step` is stated in the unit the engine stores, so it travels
  // with the value; the per-unit default is already in display units.
  let step = field.step === undefined ? shown.step : toDisplay(field.step, scale);
  if (field.type === 'integer' && scale === 1) {
    step = Math.max(1, Math.round(step));
  }

  const offers = family && FAMILIES[family].offers.length > 1 ? family : undefined;
  return offers
    ? { unit: shown.unit, step, scale, family: offers, options: familyOptions(offers) }
    : { unit: shown.unit, step, scale };
}

/** The unit a family is currently shown in: the user's choice, or its default. */
export function displayUnitOf(family: string, display: UnitDisplay = {}): string {
  const chosen = display[family];
  return offersUnit(family, chosen) ? chosen : FAMILIES[family].fallback;
}

/**
 * Stored → shown. Converting lands on values like 24.999999999999996, so the
 * result is trimmed to a precision no control ever needs.
 */
export function toDisplay(stored: number, scale: number): number {
  return scale === 1 ? stored : Math.round(stored * scale * 1e6) / 1e6;
}

/**
 * Shown → stored. The engine reads the stored form, so convert before emitting.
 *
 * Kept three decimals finer than {@link toDisplay} on purpose. A value typed in
 * one unit and stored in another rarely divides evenly — `9001 mm/min` is
 * `150.016666… mm/s` — and trimming the stored number at the display precision
 * leaves enough error to show back as `9001.00002` in the box the user just
 * typed `9001` into. Rounding the round trip's two halves at different
 * precisions is what makes the wider one absorb the narrower one's error.
 */
export function toStored(shown: number, scale: number): number {
  return scale === 1 ? shown : Math.round((shown / scale) * 1e9) / 1e9;
}
