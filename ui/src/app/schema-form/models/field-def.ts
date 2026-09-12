export interface EnumOption {
  value: string;
  /** Human-friendly label for the value; falls back to the raw value. */
  label: string;
  description?: string;
}

/**
 * `array` is carried through rather than collapsed into `string` so the form can
 * *skip* it. Fan curves and pause triggers are structured lists with editors of
 * their own; folding them into the string bucket handed them to the fallback
 * widget, which rendered a number spinner for a list of objects.
 */
export type FieldType = 'number' | 'integer' | 'boolean' | 'string' | 'array';

/**
 * Conditional relevance rule for a field, mirroring the `x-relevant-when`
 * JSON Schema extension emitted by the backend. The field is only relevant
 * (and therefore rendered) when the sibling field named `field` currently
 * satisfies the condition.
 *
 * Exactly one operator is expected per rule; `equals` wins if both are given.
 * The shape intentionally leaves room for an `in?: unknown[]` variant to be
 * added later without a breaking change.
 */
export interface FieldRelevance {
  /** Key of the sibling gate field whose value is inspected. */
  field: string;
  /** The field is relevant when the gate value strictly equals this scalar. */
  equals?: unknown;
  /**
   * The field is relevant when the gate value is a number strictly greater
   * than this one. Gates a setting that only means something once a numeric
   * feature is switched on by a non-zero amount — an elephant-foot taper is
   * inert while the compensation itself is `0`.
   */
  greaterThan?: number;
}

export interface FieldDef {
  key: string;
  type: FieldType;
  /** Raw JSON Schema format hint (e.g. "double", "uint"). */
  format?: string;
  /** Human-readable label. Falls back to key if absent. */
  title?: string;
  /** Markdown-formatted description from the schema. */
  description?: string;
  default?: unknown;
  required: boolean;
  minimum?: number;
  maximum?: number;
  /** x-group value from the schema, used for visual grouping. */
  group?: string;
  /**
   * `x-tier` schema extension: how much the user has to know before this
   * setting is worth showing them.
   *
   * Absent means *everyday* — the decisions a print actually depends on, and
   * what the panel shows before the user asks for more. `advanced` is a real
   * choice a user can form an intention about ("keep the seam at the back");
   * `expert` is an algorithm knob almost nobody can predict the effect of.
   *
   * A tier governs what is shown **by default**, never what exists: search
   * spans every tier, and a tier that hid a setting from search would have
   * become a feature flag.
   */
  tier?: 'advanced' | 'expert';
  /**
   * `x-unit` schema extension: how the stored number relates to what the user
   * reads. The engine keeps most proportions as a fraction — `fan_speed: 1.0`
   * is full speed — which is not what a box suffixed `%` appears to say.
   *
   * - `fraction` — stored 0–1, shown as 0–100 %.
   * - `percent` — already on a 0–100 scale, and free to exceed it.
   * - `ratio` — a multiplier against something else (nozzle diameter, nominal
   *   flow), shown with `×` because it is not a proportion of a whole.
   */
  unit?: string;
  /**
   * `x-step` schema extension: the increment for this field's control, where
   * the unit's default is wrong for its working range. A layer height lives
   * near 0.2 and needs 0.01; a skirt distance near 200 would take a lifetime to
   * reach at that increment.
   */
  step?: number;
  /**
   * `x-widget` schema extension: an explicit widget hint that overrides the
   * default control chosen from the field's shape. E.g. `"gcode"` selects a
   * code editor for a multiline G-code string that would otherwise fall through
   * to a plain text/number input.
   */
  widget?: string;
  /** Populated when the field is an enum type. */
  enumOptions?: EnumOption[];
  /**
   * Conditional relevance rule from the `x-relevant-when` schema extension.
   * When present, the field is only rendered while the gate condition holds
   * against the current form value; when absent the field is always relevant.
   */
  relevantWhen?: FieldRelevance;
}

export interface SchemaGroup {
  name: string;
  fields: FieldDef[];
}
