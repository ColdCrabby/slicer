import { enumLabel, fieldLabel } from './field-labels';
import { EnumOption, FieldDef, FieldRelevance, FieldType, SchemaGroup } from './field-def';

type RawProp = Record<string, unknown>;
type RawDefs = Record<string, { oneOf?: Array<{ const?: unknown; description?: string }> }>;

const UNGROUPED = 'General';

/**
 * Read the `x-relevant-when` extension from a raw property, defensively.
 * Returns a `FieldRelevance` only when the value is an object carrying a
 * string `field`; otherwise `undefined` (the field is always relevant).
 */
function resolveRelevantWhen(prop: RawProp): FieldRelevance | undefined {
  const raw = prop['x-relevant-when'];
  if (!raw || typeof raw !== 'object') {
    return undefined;
  }
  const rule = raw as Record<string, unknown>;
  if (typeof rule['field'] !== 'string') {
    return undefined;
  }
  return {
    field: rule['field'],
    equals: rule['equals'],
    greaterThan: typeof rule['greaterThan'] === 'number' ? rule['greaterThan'] : undefined,
  };
}

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
 * Whether a property is a namespace — an object with its own `properties` —
 * rather than a leaf control.
 *
 * Only plugin settings are shaped this way today (`plugins.<id>.<key>`); they
 * are namespaced so a plugin can never collide with one of the hundred-odd
 * core keys. An object property with *no* declared properties is not a
 * namespace but an open bag — a build that ships no plugins has one — and it
 * contributes no fields at all rather than a stray text input labelled after
 * the bag.
 */
function isNamespace(prop: RawProp): boolean {
  const nested = prop['properties'];
  return (
    prop['type'] === 'object' &&
    !!nested &&
    typeof nested === 'object' &&
    Object.keys(nested as object).length > 0
  );
}

/**
 * Build the `FieldDef`s one schema property contributes.
 *
 * A leaf yields exactly one, keyed by its own name — the flat case, unchanged.
 * A namespace recurses, and its children are keyed by dotted path so the form
 * can read and write them with {@link valueAtPath} / {@link patchForPath}.
 */
function fieldsFor(
  key: string,
  prop: RawProp,
  defs: RawDefs,
  required: Set<string>,
  prefix = '',
): FieldDef[] {
  const path = prefix ? `${prefix}.${key}` : key;

  if (isNamespace(prop)) {
    const nested = prop['properties'] as Record<string, RawProp>;
    const nestedRequired = new Set((prop['required'] as string[] | undefined) ?? []);
    return Object.entries(nested).flatMap(([childKey, childProp]) =>
      fieldsFor(childKey, childProp, defs, nestedRequired, path),
    );
  }

  // An object with no properties is an open bag, not a control.
  if (prop['type'] === 'object') {
    return [];
  }

  return [
    {
      key: path,
      type: resolveFieldType(prop),
      format: prop['format'] as string | undefined,
      title: (prop['title'] as string | undefined) ?? fieldLabel(key),
      description: prop['description'] as string | undefined,
      default: prop['default'],
      required: required.has(key),
      minimum: prop['minimum'] as number | undefined,
      maximum: prop['maximum'] as number | undefined,
      group: prop['x-group'] as string | undefined,
      widget: prop['x-widget'] as string | undefined,
      enumOptions: resolveEnumOptions(prop, defs),
      relevantWhen: resolveRelevantWhen(prop),
    },
  ];
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

  const fields: FieldDef[] = Object.entries(properties).flatMap(([key, prop]) =>
    fieldsFor(key, prop, defs, required),
  );

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
