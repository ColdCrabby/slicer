import { FieldDef, SchemaGroup } from './field-def';

/**
 * Shared evaluation of the `x-relevant-when` schema extension.
 *
 * These helpers are consumed by both the {@link SchemaForm} component and the
 * profile editor pages (filaments/printers/profiles) so every schema-driven
 * surface hides gated-off fields with identical semantics.
 */

/**
 * Evaluate a field's `x-relevant-when` gate against the current form values.
 *
 * Returns `true` when the field has no relevance rule (always relevant), or
 * when its gate condition is satisfied. Only scalar `equals` is supported:
 * the gate field's raw current value is compared with strict equality against
 * `equals`. A missing gate value therefore counts as "not equal" unless the
 * rule's `equals` is itself `undefined`.
 */
export function isFieldRelevant(field: FieldDef, values: Record<string, unknown>): boolean {
  const rule = field.relevantWhen;
  if (!rule) {
    return true;
  }
  return values[rule.field] === rule.equals;
}

/**
 * Filter a list of groups down to only the fields that are currently relevant
 * given `values`, dropping any group that loses all of its fields.
 *
 * Pure and non-mutating — returns fresh group objects so callers can safely
 * memoise the result. Field order within each group is preserved.
 */
export function filterRelevantGroups(
  groups: SchemaGroup[],
  values: Record<string, unknown>,
): SchemaGroup[] {
  return groups
    .map((group) => ({
      name: group.name,
      fields: group.fields.filter((f) => isFieldRelevant(f, values)),
    }))
    .filter((group) => group.fields.length > 0);
}

/**
 * Collect each field's schema `default` into a flat `key → default` record.
 *
 * Profile `params` bags are *sparse* — an absent key resolves to the engine
 * default at slice time. A relevance gate must therefore be evaluated against
 * the *effective* values (defaults overlaid with the profile's own overrides),
 * otherwise a gate whose field is unset (e.g. `adhesion_type` still at its
 * `none` default) can't be reasoned about. Callers merge this map underneath
 * the sparse overrides before calling {@link filterRelevantGroups}.
 */
export function collectFieldDefaults(fields: FieldDef[]): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const field of fields) {
    if (field.default !== undefined) {
      out[field.key] = field.default;
    }
  }
  return out;
}
