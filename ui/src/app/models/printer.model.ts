import type {
  PrinterProfile,
  PrinterConnection,
} from '../../generated/slicer-engine-ws-client-message-v1';
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

const DEFAULT_START_GCODE = `; --- start ---
G28 ; home all axes
G92 E0 ; reset extruder
G1 Z2.0 F3000 ; lift nozzle`;

const DEFAULT_END_GCODE = `; --- end ---
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

/** Printable-area dimensions for the {@link PrintArea} config. */
export function printerBedConfig(printer: PrinterProfile): {
  printableAreaWidth: number;
  printableAreaHeight: number;
} {
  return {
    printableAreaWidth: printer.bed_width,
    printableAreaHeight: printer.bed_shape === 'circular' ? printer.bed_width : printer.bed_depth,
  };
}
