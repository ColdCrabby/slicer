import globalSettingsSchema from '../../schemas/slicer-engine-global-settings-v1.json';

/**
 * Which setting each proportional setting is a proportion *of*, read out of the
 * schema's `x-derived-from` annotations so the engine's `DERIVED_FROM` stays
 * the only list of them.
 */
const DERIVED_FROM: ReadonlyMap<string, string> = new Map(
  Object.entries(
    (globalSettingsSchema.$defs.SlicingParams as { properties: Record<string, unknown> })
      .properties,
  ).flatMap(([key, spec]) => {
    const base = (spec as Record<string, unknown>)['x-derived-from'];
    return typeof base === 'string' ? [[key, base] as [string, string]] : [];
  }),
);

/** `"110%"` → `1.1`. Anything else — including a plain number — is `null`. */
function parsePercent(value: unknown): number | null {
  if (typeof value !== 'string') {
    return null;
  }
  const text = value.trim();
  if (!text.endsWith('%')) {
    return null;
  }
  const n = Number(text.slice(0, -1).trim());
  return Number.isFinite(n) ? n / 100 : null;
}

/**
 * Resolve every `"NN%"` in a merged params document into a number, mirroring
 * the engine's `resolve_derived_values`.
 *
 * A bead width pinned in millimetres is right on one nozzle and wrong on the
 * next, so a process profile shared across machines states a proportion
 * instead. Every consumer downstream — the form, the diff against the presets,
 * the write-back — reads plain numbers, exactly as the engine's does.
 *
 * Runs on the *merged* document, so a proportion resolves against the nozzle
 * that actually won rather than the one the profile stating it sat beside. A
 * proportion whose base is missing is left alone rather than guessed at.
 */
export function resolveDerivedValues(params: Record<string, unknown>): Record<string, unknown> {
  const resolved = { ...params };
  for (const [key, baseKey] of DERIVED_FROM) {
    const percent = parsePercent(resolved[key]);
    if (percent === null) {
      continue;
    }
    const base = resolved[baseKey];
    if (typeof base === 'number' && Number.isFinite(base)) {
      resolved[key] = percent * base;
    }
  }
  return resolved;
}
