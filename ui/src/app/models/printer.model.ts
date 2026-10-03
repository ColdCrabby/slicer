import type {
  PrinterProfile,
  PrinterConnection,
} from '../../generated/slicer-engine-ws-client-message-v1';
import type { SlicingParams } from '../../generated/slicer-engine-printer-profile-v1';
import globalSettingsSchema from '../../schemas/slicer-engine-global-settings-v1.json';
import { enumLabel } from '../schema-form/models/field-labels';
import type { SceneBedSnapshot } from '../services/scene-engine';
import type { FilamentMaterial } from './filament.model';
import {
  DEFAULT_GCODE_TEMPLATE_ID,
  defaultGcodeTemplateIdForFlavor,
  gcodeTemplatePatch,
} from './gcode-templates';
import { uid } from './id';

/**
 * Printer (machine) profile.
 *
 * This is the engine's own type (generated from the Rust `PrinterProfile`).
 * Hardware/domain fields live at the top level; every *slice* parameter the
 * printer contributes lives in {@link PrinterProfile.params} as a partial
 * `SlicingParams` — the same field names and units the slicer uses. There is no
 * separate camelCase model and no mapping layer: the object built here is sent
 * to the slicer as-is.
 */
export type { PrinterProfile, PrinterConnection };

export type PrinterConnectionKind = NonNullable<PrinterConnection['kind']>;
export type PrinterGcodeFlavor = NonNullable<SlicingParams['gcode_flavor']>;
export type BedShape = NonNullable<PrinterProfile['bed_shape']>;

/**
 * Which layer of the profile stack supplied a resolved setting. Mirrors the
 * engine's `ParamOrigin`.
 */
export type ParamOrigin =
  'default' | 'printer' | 'filament' | 'process' | 'machine_material' | 'override';

/**
 * What this machine does differently with one family of material — a sparse
 * `SlicingParams` patch, or an empty object when it has nothing to say.
 *
 * Keyed on the material *family* rather than a filament id on purpose: a
 * correction is stated once per machine and every spool of that material
 * inherits it, so a new spool never costs one profile per printer. The
 * settings eligible for one are the closed set the schema marks
 * `x-per-machine-material`.
 */
export function materialOverlayOf(
  printer: PrinterProfile,
  material: FilamentMaterial,
): Record<string, unknown> {
  const overlays = printer.material_overlays as Record<string, Record<string, unknown>> | undefined;
  return overlays?.[material] ?? {};
}

export const PRINTER_CONNECTION_LABELS: Record<PrinterConnectionKind, string> = {
  none: 'Not connected',
  octoprint: 'OctoPrint',
  moonraker: 'Moonraker (Klipper)',
  bambu: 'Bambu Lab',
  prusalink: 'PrusaLink',
};

export const PRINTER_CONNECTION_KINDS: PrinterConnectionKind[] = [
  'none',
  'octoprint',
  'moonraker',
  'bambu',
  'prusalink',
];

/**
 * The firmware flavors a printer can run, in the engine's order.
 *
 * Read from the generated schema rather than listed here: the engine's
 * `GcodeFlavor` is the one list, so a dialect it adds is offered with no UI
 * change, and one it drops can never linger as a choice the slicer refuses.
 */
export const PRINTER_GCODE_FLAVORS: { value: PrinterGcodeFlavor; label: string }[] = (
  globalSettingsSchema.$defs.GcodeFlavor.oneOf as { const: PrinterGcodeFlavor }[]
).map(({ const: value }) => ({ value, label: enumLabel(value) }));

/** The flavor `value` names, or `undefined` for anything the engine lacks. */
export function asGcodeFlavor(value: string | undefined): PrinterGcodeFlavor | undefined {
  const token = value?.trim().toLowerCase();
  return PRINTER_GCODE_FLAVORS.find((f) => f.value === token)?.value;
}

/** Default hardware slice params contributed by a from-scratch printer. */
export function defaultPrinterParams(): Record<string, unknown> {
  return {
    nozzle_diameter_mm: 0.4,
    filament_diameter_mm: 1.75,
    extruder_count: 1,
    heated_chamber: false,
    print_speed: 150,
    travel_speed_mm_min: 15000,
    retract_mm: 0.8,
    retract_speed_mm_min: 2400,
    z_hop_mm: 0.2,
    // Blocks + template association (id/rev) so edits show "Modified from …".
    ...gcodeTemplatePatch(DEFAULT_GCODE_TEMPLATE_ID),
  };
}

/** Sensible blank-slate printer used when creating one from scratch. */
export function makePrinter(overrides: Partial<PrinterProfile> = {}): PrinterProfile {
  return {
    id: uid(),
    name: 'New printer',
    source: 'user',
    vendor: 'Custom',
    model: 'Generic',
    bed_shape: 'rectangular',
    bed_width: 220,
    bed_depth: 220,
    bed_height: 250,
    origin_at_center: false,
    preferred_orientation_deg: 0,
    connection: { kind: 'none', connected: false },
    params: defaultPrinterParams(),
    ...overrides,
  };
}

/** The offline default printer — what every fallback resolves to. */
export const DEFAULT_PRINTER: PrinterProfile = makePrinter({
  id: 'builtin-generic-printer',
  name: 'Generic 220 mm printer',
  source: 'builtin',
  vendor: 'Generic',
  model: 'FDM 220',
});

/**
 * A generic high-performance CoreXY — the class, not a model.
 *
 * It exists because the fast print profiles are unusable behind a printer that
 * travels at 250 mm/s and retracts 0.8 mm: a process can ask for speed the
 * printer profile then refuses to carry.
 *
 * **No pressure advance, and firmware retraction on.** Both are calibrated on
 * the machine and live in its firmware; a preset shipping numbers for them
 * would overwrite a calibration it knows nothing about. The retraction lengths
 * below are only the fallback for a firmware that does not answer `G10`/`G11`.
 */
export const COREXY_PRINTER: PrinterProfile = makePrinter({
  id: 'builtin-corexy-350',
  name: 'Generic CoreXY 350 mm',
  source: 'builtin',
  vendor: 'Generic',
  model: 'CoreXY 350',
  bed_width: 350,
  bed_depth: 350,
  bed_height: 370,
  params: {
    ...defaultPrinterParams(),
    nozzle_diameter_mm: 0.6,
    gcode_flavor: 'klipper',
    travel_speed_mm_min: 36000,
    use_firmware_retraction: true,
    retract_mm: 0.4,
    retract_speed_mm_min: 1800,
    // A failed part can be skipped without losing the plate.
    exclude_object: true,
    ...gcodeTemplatePatch(defaultGcodeTemplateIdForFlavor('klipper')),
  },
});

/** The built-in printers: a 220 mm bedslinger and a 350 mm CoreXY. */
export const DEFAULT_PRINTERS: PrinterProfile[] = [DEFAULT_PRINTER, COREXY_PRINTER];

/** Shared bed dimensions derived from a printer profile. */
function resolvedBedFootprint(printer: PrinterProfile): { width: number; depth: number } {
  const width = printer.bed_width ?? 220;
  const depth = printer.bed_shape === 'circular' ? width : (printer.bed_depth ?? 220);
  return { width, depth };
}

/** Printable-area dimensions for the {@link PrintArea} config. */
export function printerBedConfig(printer: PrinterProfile): {
  bedShape: 'rectangular' | 'circular';
  printableAreaWidth: number;
  printableAreaHeight: number;
  movableAreaX: number;
  movableAreaY: number;
} {
  const footprint = resolvedBedFootprint(printer);
  const movableAreaX = printer.origin_at_center ? -footprint.width / 2 : 0;
  const movableAreaY = printer.origin_at_center ? -footprint.depth / 2 : 0;
  return {
    bedShape: printer.bed_shape === 'circular' ? 'circular' : 'rectangular',
    printableAreaWidth: footprint.width,
    printableAreaHeight: footprint.depth,
    movableAreaX,
    movableAreaY,
  };
}

/** Scene-engine bed config used by bed-aware ops (`CenterOnBed`, packing, etc). */
export function printerSceneBedConfig(printer: PrinterProfile): SceneBedSnapshot {
  const footprint = resolvedBedFootprint(printer);
  const origin_offset_x = printer.origin_at_center ? -footprint.width / 2 : 0;
  const origin_offset_y = printer.origin_at_center ? -footprint.depth / 2 : 0;
  return {
    width: footprint.width,
    depth: footprint.depth,
    height: printer.bed_height ?? 250,
    origin_offset_x,
    origin_offset_y,
    shape: printer.bed_shape === 'circular' ? 'circular' : 'rectangular',
  };
}
