export interface EnumOption {
  value: string;
  /** Human-friendly label for the value; falls back to the raw value. */
  label: string;
  description?: string;
}

export type FieldType = 'number' | 'integer' | 'boolean' | 'string';

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
