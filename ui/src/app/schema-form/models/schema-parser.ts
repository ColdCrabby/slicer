import { enumLabel, fieldLabel } from './field-labels';
import { EnumOption, FieldDef, FieldType, SchemaGroup } from './field-def';

type RawProp = Record<string, unknown>;
type RawDefs = Record<string, { oneOf?: Array<{ const?: unknown; description?: string }> }>;

const UNGROUPED = 'General';

/**
 * Resolve enum options from a property that either has a direct `oneOf` array
 * or references a `$defs` entry via `$ref`.
 */
function resolveEnumOptions(prop: RawProp, defs: RawDefs): EnumOption[] | undefined {
  if ('$ref' in prop) {
    const ref = prop['$ref'] as string;
    const defName = ref.replace('#/$defs/', '');
    const def = defs[defName];
    if (def?.oneOf) {
      return def.oneOf.map((v) => ({
        value: String(v.const),
        label: enumLabel(String(v.const)),
        description: v.description,
      }));
    }
  }

  if ('oneOf' in prop) {
    const oneOf = prop['oneOf'] as Array<{ const?: unknown; description?: string }>;
    return oneOf.map((v) => ({
      value: String(v.const),
      label: enumLabel(String(v.const)),
      description: v.description,
    }));
  }

  return undefined;
}

/**
 * Map a JSON Schema type + format combination to the normalised FieldType.
 */
function resolveFieldType(prop: RawProp): FieldType {
  const type = prop['type'] as string | undefined;
  if (type === 'boolean') {
    return 'boolean';
  }
  if (type === 'integer') {
    return 'integer';
  }
  if (type === 'number') {
    return 'number';
  }
  return 'string';
}

/**
 * Parse a JSON Schema object into grouped `FieldDef` entries.
 *
 * @param schema  A raw JSON Schema object. The function looks for `properties`
 *                directly on the schema or on a `$ref`-resolved `$defs` entry.
 * @param rootDefs  Optional pre-extracted `$defs` map. When absent the
 *                  function falls back to `schema.$defs` if present.
 */
export function parseSchema(
  schema: Record<string, unknown>,
  rootDefs?: Record<string, unknown>,
): { groups: SchemaGroup[]; fields: FieldDef[] } {
  const defs = (rootDefs ?? (schema['$defs'] as RawDefs | undefined) ?? {}) as RawDefs;

  let properties: Record<string, RawProp> | undefined;
  let required: Set<string> = new Set();

  // The schema may expose properties directly, or have a single $ref at root
  // pointing to a $defs entry (e.g. the global-settings schema wraps SlicingParams).
  if ('properties' in schema) {
    properties = schema['properties'] as Record<string, RawProp>;
    required = new Set((schema['required'] as string[] | undefined) ?? []);
  } else if ('$ref' in schema) {
    const ref = (schema['$ref'] as string).replace('#/$defs/', '');
    const def = defs[ref] as Record<string, unknown> | undefined;
    if (def && 'properties' in def) {
      properties = def['properties'] as Record<string, RawProp>;
      required = new Set((def['required'] as string[] | undefined) ?? []);
    }
  }

  if (!properties) {
    return { groups: [], fields: [] };
  }

  const fields: FieldDef[] = Object.entries(properties).map(([key, prop]) => {
    const fieldDef: FieldDef = {
      key,
      type: resolveFieldType(prop),
      format: prop['format'] as string | undefined,
      title: (prop['title'] as string | undefined) ?? fieldLabel(key),
      description: prop['description'] as string | undefined,
      default: prop['default'],
      required: required.has(key),
      minimum: prop['minimum'] as number | undefined,
      maximum: prop['maximum'] as number | undefined,
      group: prop['x-group'] as string | undefined,
      enumOptions: resolveEnumOptions(prop, defs),
    };
    return fieldDef;
  });

  // Group fields, preserving insertion order within each group.
  const groupMap = new Map<string, FieldDef[]>();
  for (const field of fields) {
    const name = field.group ?? UNGROUPED;
    if (!groupMap.has(name)) {
      groupMap.set(name, []);
    }
    groupMap.get(name)!.push(field);
  }

  const groups: SchemaGroup[] = Array.from(groupMap.entries()).map(([name, groupFields]) => ({
    name,
    fields: groupFields,
  }));

  return { groups, fields };
}
