import globalSettingsSchema from '../../schemas/slicer-engine-global-settings-v1.json';
import type { SlicingParams } from '../../generated/slicer-engine-ws-client-message-v1';

export type { SlicingParams as SliceSettings } from '../../generated/slicer-engine-ws-client-message-v1';

/** The one part of a generated schema property this module reads. */
interface SchemaProp {
  default?: unknown;
}

/**
 * Every slicing parameter's engine default, read straight out of the generated
 * JSON Schema.
 *
 * The schema is emitted from `SlicingParams` by `gen-schemas`, so this map *is*
 * the Rust defaults — all 170-odd of them, including the ones no hand-written
 * list would have remembered. It replaced a literal carrying a dozen keys that
 * had already drifted from the engine (its nozzle temperature was five degrees
 * off), which matters because this is the floor the override diff is measured
 * against: a key missing here reads as "the user set it" and would be sent on
 * every slice.
 *
 * Display and diffing only — the engine re-resolves the whole stack at slice
 * time. See {@link ../services/profiles/active-selection.ActiveSelection}.
 */
export const ENGINE_DEFAULTS: Readonly<Partial<SlicingParams>> = Object.freeze(
  Object.fromEntries(
    Object.entries(
      (globalSettingsSchema.$defs.SlicingParams as { properties: Record<string, SchemaProp> })
        .properties,
    )
      .filter(([, prop]) => prop.default !== undefined)
      .map(([key, prop]) => [key, prop.default]),
  ),
) as Partial<SlicingParams>;

/**
 * Structural equality for a settings value. JSON is enough: every
 * `SlicingParams` field is a scalar, a string, or a plain array/object of them,
 * and `undefined` and `null` both mean "the profile said nothing".
 */
function sameSettingValue(a: unknown, b: unknown): boolean {
  return a === b || JSON.stringify(a ?? null) === JSON.stringify(b ?? null);
}

/**
 * Fold a settings change into an override diff, measured against `baseline` —
 * the resolved `defaults → printer → filament → process` stack.
 *
 * A value that matches the baseline is *removed* rather than stored: dragging a
 * slider back to where the profile had it must leave the setting inheriting
 * again. Storing it would pin the plate to today's value and stop it following
 * the profile it was never really changed away from — the difference only shows
 * up weeks later, when editing a process profile fails to move the one plate
 * the user had happened to touch that setting on.
 *
 * Returns a new object; the input is never mutated.
 */
export function applyOverridePatch(
  overrides: Readonly<Record<string, unknown>>,
  patch: Readonly<Record<string, unknown>>,
  baseline: Readonly<Record<string, unknown>>,
): Record<string, unknown> {
  const next = { ...overrides };
  for (const [key, value] of Object.entries(patch)) {
    if (sameSettingValue(value, baseline[key])) {
      delete next[key];
    } else {
      next[key] = value;
    }
  }
  return next;
}
