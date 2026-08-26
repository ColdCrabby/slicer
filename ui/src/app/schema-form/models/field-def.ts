export interface EnumOption {
  value: string;
  /** Human-friendly label for the value; falls back to the raw value. */
  label: string;
  description?: string;
}

export type FieldType = 'number' | 'integer' | 'boolean' | 'string';

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
  /** Populated when the field is an enum type. */
  enumOptions?: EnumOption[];
}

export interface SchemaGroup {
  name: string;
  fields: FieldDef[];
}
