/**
 * Typed accessors for a profile's `params` bundle (a partial `SlicingParams`
 * carried as a loosely-typed object). These let templates read a value with a
 * concrete type instead of an untyped cast.
 */

/** Read a numeric slice param, defaulting to `0` when absent. */
export function paramNum(params: unknown, key: string): number {
  const value = (params as Record<string, unknown> | null | undefined)?.[key];
  return typeof value === 'number' ? value : Number(value ?? 0);
}

/** Read a string/enum slice param, defaulting to `''` when absent. */
export function paramStr(params: unknown, key: string): string {
  const value = (params as Record<string, unknown> | null | undefined)?.[key];
  return value == null ? '' : String(value);
}

/** Read a boolean slice param. */
export function paramBool(params: unknown, key: string): boolean {
  return Boolean((params as Record<string, unknown> | null | undefined)?.[key]);
}
