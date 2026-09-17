import type { FilamentMaterial } from '../../models/filament.model';
import type { PrinterProfile } from '../../models/printer.model';

/**
 * Editing a machine's per-material corrections.
 *
 * Pure functions over the overlay map, shared by the two places that write it:
 * the write-back dialog, which *captures* a correction from a plate that
 * printed wrong, and the printer editor, which manages the ones already there.
 * Both needed the same merge, and a second copy of it is how one of them would
 * eventually start dropping the other materials.
 */

/** One material's corrections, never null. */
export function correctionsFor(
  printer: PrinterProfile,
  material: FilamentMaterial | string,
): Record<string, unknown> {
  const overlays = (printer.material_overlays ?? {}) as Record<string, Record<string, unknown>>;
  return overlays[material] ?? {};
}

/**
 * This machine's correction map with one material's entry replaced by `next`.
 *
 * Two rules, both of which exist to stop an edit meaning more than it said:
 *
 * - **Every other material is left exactly as it was.** A machine usually
 *   corrects more than one, and fixing PLA's flow must not lose what was
 *   already measured about ABS.
 * - **A material left with nothing is dropped**, not kept as an empty entry.
 *   An empty correction is not a value of zero — it is the material's own value
 *   standing again, which is what its absence means everywhere else.
 */
export function withCorrections(
  printer: PrinterProfile,
  material: FilamentMaterial | string,
  next: Record<string, unknown>,
): Record<string, unknown> {
  const overlays = { ...((printer.material_overlays ?? {}) as Record<string, unknown>) };
  if (Object.keys(next).length > 0) {
    overlays[material] = next;
  } else {
    delete overlays[material];
  }
  return overlays;
}

/** One material's corrections with `patch` applied on top. */
export function mergedCorrections(
  printer: PrinterProfile,
  material: FilamentMaterial | string,
  patch: Record<string, unknown>,
): Record<string, unknown> {
  return { ...correctionsFor(printer, material), ...patch };
}

/** One material's corrections without `key` — that setting is inherited again. */
export function withoutCorrection(
  printer: PrinterProfile,
  material: FilamentMaterial | string,
  key: string,
): Record<string, unknown> {
  const next = correctionsFor(printer, material);
  const copy = { ...next };
  delete copy[key];
  return copy;
}

/** Material families this machine corrects, in the order they were recorded. */
export function correctedMaterials(printer: PrinterProfile): string[] {
  const overlays = (printer.material_overlays ?? {}) as Record<string, Record<string, unknown>>;
  return Object.keys(overlays).filter(
    (material) => Object.keys(overlays[material] ?? {}).length > 0,
  );
}
