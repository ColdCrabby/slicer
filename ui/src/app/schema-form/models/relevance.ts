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
 * when its gate condition is satisfied. Two operators are supported:
 *
 * - `equals` — the gate field's raw current value compared with strict
 *   equality. A missing gate value therefore counts as "not equal" unless the
 *   rule's `equals` is itself `undefined`.
 * - `greaterThan` — the gate field's value coerced to a number and compared.
 *   A missing or non-numeric value is not greater than anything, so the field
 *   stays hidden.
 *
 * `equals` wins when a rule carries both.
 */
export function isFieldRelevant(field: FieldDef, values: Record<string, unknown>): boolean {
  const rule = field.relevantWhen;
  if (!rule) {
    return true;
  }
  if (rule.equals === undefined && rule.greaterThan !== undefined) {
    const value = Number(values[rule.field]);
    return Number.isFinite(value) && value > rule.greaterThan;
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
