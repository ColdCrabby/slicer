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

/**
 * Disclosure tiers, ordered from what everyone sees to what only a specialist
 * goes looking for. The index is the comparison: a view revealing `advanced`
 * shows everything at or below it.
 */
export const TIER_ORDER = ['everyday', 'advanced', 'expert'] as const;

export type Tier = (typeof TIER_ORDER)[number];

/** A field's tier, defaulting to `everyday` for anything the schema left bare. */
export function tierOf(field: FieldDef): Tier {
  return field.tier ?? 'everyday';
}

/**
 * Whether `field` is shown when a section is revealed up to `revealed`.
 *
 * Lives here beside `isFieldRelevant` because the two answer the same shape of
 * question — "should this be on screen right now?" — and every schema-driven
 * surface has to agree on both. Relevance is about the *state* of the plate;
 * tier is about how far the user has asked to look.
 */
export function isFieldInTier(field: FieldDef, revealed: Tier): boolean {
  return TIER_ORDER.indexOf(tierOf(field)) <= TIER_ORDER.indexOf(revealed);
}

/** The deepest tier present in `fields`, or `everyday` when there is nothing more. */
export function deepestTier(fields: readonly FieldDef[]): Tier {
  let deepest: Tier = 'everyday';
  for (const field of fields) {
    if (TIER_ORDER.indexOf(tierOf(field)) > TIER_ORDER.indexOf(deepest)) {
      deepest = tierOf(field);
    }
  }
  return deepest;
}

/**
 * The *shallowest* tier present in `fields` — the point at which a group first
 * has something to show.
 *
 * A group whose every field is advanced has nothing to put on screen in the
 * everyday view, so listing its header there offers the user a section that
 * opens onto nothing. This is what lets the panel hide the whole group until
 * the tier it belongs to is revealed.
 */
export function shallowestTier(fields: readonly FieldDef[]): Tier {
  let shallowest: Tier = 'expert';
  for (const field of fields) {
    if (TIER_ORDER.indexOf(tierOf(field)) < TIER_ORDER.indexOf(shallowest)) {
      shallowest = tierOf(field);
    }
  }
  return fields.length === 0 ? 'everyday' : shallowest;
}

/**
 * Fields ordered simple-to-complex: everyday first, then advanced, then expert,
 * keeping the schema's own order inside each tier.
 *
 * This is what makes a disclosure *append*. Filtering alone leaves a revealed
 * field wherever the Rust struct happens to declare it, so pressing "Advanced"
 * inserted controls between the ones already on screen and slid everything the
 * user was looking at down the panel. Sorting first means the new block always
 * starts below the last thing that was visible.
 *
 * The sort is stable, so a group's own field order still says what goes next to
 * what — it only ever moves a field later, never past a peer of its own tier.
 */
export function orderFieldsByTier(fields: readonly FieldDef[]): FieldDef[] {
  return [...fields].sort((a, b) => TIER_ORDER.indexOf(tierOf(a)) - TIER_ORDER.indexOf(tierOf(b)));
}

/**
 * Groups ordered the same way, by the shallowest tier each one contains.
 *
 * The panel hides a group whose every field sits deeper than the reader has
 * asked, so the same insertion problem applies one level up: `Quality` is
 * advanced-only and sits between `Speed` and `Surfaces` in the taxonomy, and
 * revealing advanced sections used to push half the panel down to make room
 * for it.
 *
 * Stable again — the contract's taxonomy order survives within a tier.
 */
export function orderGroupsByTier(groups: readonly SchemaGroup[]): SchemaGroup[] {
  return [...groups].sort(
    (a, b) =>
      TIER_ORDER.indexOf(shallowestTier(a.fields)) - TIER_ORDER.indexOf(shallowestTier(b.fields)),
  );
}

/** Whether `tier` is at or above `revealed` in the disclosure order. */
export function isTierAtMost(tier: Tier, revealed: Tier): boolean {
  return TIER_ORDER.indexOf(tier) <= TIER_ORDER.indexOf(revealed);
}

/** Whichever of the two tiers reveals more. */
export function deeperOf(a: Tier, b: Tier): Tier {
  return TIER_ORDER.indexOf(a) >= TIER_ORDER.indexOf(b) ? a : b;
}

/** The tier one step beyond `revealed`, or `null` when already at the deepest. */
export function nextTier(revealed: Tier): Tier | null {
  const index = TIER_ORDER.indexOf(revealed);
  return index < TIER_ORDER.length - 1 ? TIER_ORDER[index + 1] : null;
}
