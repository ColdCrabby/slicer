import type {
  PrinterProfile,
  PrinterConnection,
} from '../../generated/slicer-engine-ws-client-message-v1';
import type { SceneBedSnapshot } from '../services/scene-engine';
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
export type PrinterGcodeFlavor = 'marlin' | 'klipper';
export type BedShape = NonNullable<PrinterProfile['bed_shape']>;

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

export const PRINTER_GCODE_FLAVORS: { value: PrinterGcodeFlavor; label: string }[] = [
  { value: 'marlin', label: 'Marlin' },
  { value: 'klipper', label: 'Klipper' },
];

const DEFAULT_START_GCODE = `; Nexus standard Marlin start
G21 ; millimetres
G90 ; absolute positioning
M82 ; extruder absolute mode
M140 S{bed_temp_first_layer} ; set bed temperature
M104 S{nozzle_temp_first_layer} ; set nozzle temperature
G28 ; home all axes
M190 S{bed_temp_first_layer} ; wait for bed temperature
M109 S{nozzle_temp_first_layer} ; wait for nozzle temperature
G92 E0 ; reset extruder
G1 Z2.0 F3000 ; lift nozzle`;

const DEFAULT_END_GCODE = `; Nexus standard Marlin end
G91 ; relative positioning
G1 E-2 F2700 ; retract
G1 Z10 F3000 ; lift
G90 ; absolute positioning
M104 S0 ; nozzle off
M140 S0 ; bed off
M84 ; disable steppers`;

/** Default hardware slice params contributed by a from-scratch printer. */
export function defaultPrinterParams(): Record<string, unknown> {
  return {
    nozzle_diameter_mm: 0.4,
    filament_diameter_mm: 1.75,
    extruder_count: 1,
    print_speed: 150,
    travel_speed_mm_min: 15000,
    retract_mm: 0.8,
    retract_speed_mm_min: 2400,
    z_hop_mm: 0.2,
    gcode_flavor: 'marlin',
    start_gcode: DEFAULT_START_GCODE,
    end_gcode: DEFAULT_END_GCODE,
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
    connection: { kind: 'none', connected: false },
    params: defaultPrinterParams(),
    ...overrides,
  };
}

/** The single offline default printer. */
export const DEFAULT_PRINTER: PrinterProfile = makePrinter({
  id: 'builtin-generic-printer',
  name: 'Generic 220 mm printer',
  source: 'builtin',
  vendor: 'Generic',
  model: 'FDM 220',
});

export const DEFAULT_PRINTERS: PrinterProfile[] = [DEFAULT_PRINTER];

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
