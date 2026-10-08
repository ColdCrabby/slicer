/**
 * Dotted-path access for schema field keys.
 *
 * Almost every setting is a top-level key, and for those a path is just the
 * key — these helpers are a no-op on the flat case. Plugin settings are the
 * exception: they are namespaced (`plugins.<id>.<key>`) so a plugin can never
 * collide with one of the hundred-odd core settings, which means the form has
 * to read and write one level deeper than it used to.
 *
 * Reads are defensive because a settings object is frequently partial — the
 * form falls back to the schema default for anything missing, and a plugin the
 * user has never touched has no namespace at all.
 */

/** Split a field key into its path segments. */
export function pathSegments(key: string): string[] {
  return key.split('.');
}

/** Whether a field key addresses something nested rather than a top-level key. */
export function isNestedKey(key: string): boolean {
  return key.includes('.');
}

/**
 * Read the value at `key` out of `values`, or `undefined` when any step of the
 * path is missing or is not an object.
 */
export function valueAtPath(values: Record<string, unknown>, key: string): unknown {
  const segments = pathSegments(key);
  let current: unknown = values;
  for (const segment of segments) {
    if (current === null || typeof current !== 'object') {
      return undefined;
    }
    current = (current as Record<string, unknown>)[segment];
  }
  return current;
}

/**
 * Build the shallow patch that sets `key` to `value`.
 *
 * For a top-level key this is `{ key: value }` — exactly what the callers used
 * to construct inline. For a nested key it is `{ <root>: … }` carrying a
 * rebuilt copy of that whole root branch, so the result still merges correctly
 * into a settings object with a plain spread and nothing is mutated in place.
 *
 * Intermediate objects are created as needed, so writing
 * `plugins.fuzzy-skin.enabled` works before that plugin has any settings.
 */
export function patchForPath(
  values: Record<string, unknown>,
  key: string,
  value: unknown,
): Record<string, unknown> {
  const segments = pathSegments(key);
  if (segments.length === 1) {
    return { [key]: value };
  }

  const [root, ...rest] = segments;
  const existing = values[root];
  const branch = setIn(
    existing !== null && typeof existing === 'object' ? (existing as Record<string, unknown>) : {},
    rest,
    value,
  );
  return { [root]: branch };
}

/** Recursively copy `source`, replacing the value at `segments` with `value`. */
function setIn(
  source: Record<string, unknown>,
  segments: string[],
  value: unknown,
): Record<string, unknown> {
  const [head, ...rest] = segments;
  if (rest.length === 0) {
    return { ...source, [head]: value };
  }
  const child = source[head];
  return {
    ...source,
    [head]: setIn(
      child !== null && typeof child === 'object' ? (child as Record<string, unknown>) : {},
      rest,
      value,
    ),
  };
}
