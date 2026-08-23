import type { SlicingParams } from '../../generated/slicer-engine-ws-client-message-v1';
import { uid } from './id';
import type { ProfileMeta } from './profile-source';

/** Kinds of network connection a printer can use to receive prints. */
export type PrinterConnectionKind = 'none' | 'octoprint' | 'moonraker' | 'bambu' | 'prusalink';

export interface PrinterConnection {
  kind: PrinterConnectionKind;
  host?: string;
  connected: boolean;
}

/** Firmware dialect the printer speaks — maps to the engine's `gcode_flavor`. */
export type PrinterGcodeFlavor = 'marlin' | 'klipper';

/** Bed geometry. Circular beds (deltas) use `bedWidth` as the diameter. */
export type BedShape = 'rectangular' | 'circular';

/**
 * A printer (machine) profile.
 *
 * Holds everything that is a property of the *hardware*: build volume, nozzle,
 * kinematic limits, firmware dialect, and start/end G-code. Temperatures and
 * quality live on the filament / print profiles respectively so a printer can
 * be paired with any material.
 */
export interface PrinterProfile extends ProfileMeta {
  vendor: string;
  model: string;

  // Build volume ------------------------------------------------------------
  bedShape: BedShape;
  /** Width (mm) along +X. For circular beds this is the diameter. */
  bedWidth: number;
  /** Depth (mm) along +Y. Ignored for circular beds. */
  bedDepth: number;
  /** Max Z height (mm). */
  bedHeight: number;
  /** True for delta/origin-at-center machines. */
  originAtCenter: boolean;

  // Toolhead ----------------------------------------------------------------
  nozzleDiameter: number;
  filamentDiameter: number;
  extruderCount: number;

  // Motion limits -----------------------------------------------------------
  maxPrintSpeed: number;
  maxTravelSpeed: number;
  retractionLength: number;
  retractionSpeed: number;
  zHop: number;

  // Firmware / macros -------------------------------------------------------
  gcodeFlavor: PrinterGcodeFlavor;
  startGcode: string;
  endGcode: string;

  connection: PrinterConnection;
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

/** Sensible blank-slate printer used when creating one from scratch. */
export function makePrinter(overrides: Partial<PrinterProfile> = {}): PrinterProfile {
  return {
    id: uid(),
    name: 'New printer',
    source: 'user',
    vendor: 'Custom',
    model: 'Generic',
    bedShape: 'rectangular',
    bedWidth: 220,
    bedDepth: 220,
    bedHeight: 250,
    originAtCenter: false,
    nozzleDiameter: 0.4,
    filamentDiameter: 1.75,
    extruderCount: 1,
    maxPrintSpeed: 150,
    maxTravelSpeed: 250,
    retractionLength: 0.8,
    retractionSpeed: 40,
    zHop: 0.2,
    gcodeFlavor: 'marlin',
    startGcode: DEFAULT_START_GCODE,
    endGcode: DEFAULT_END_GCODE,
    connection: { kind: 'none', connected: false },
    ...overrides,
  };
}

/**
 * The single offline default printer. Everything else (vendor machines) lives
 * in the cloud catalog so an offline install still has exactly one working
 * printer while never being blocked from making more.
 */
export const DEFAULT_PRINTER: PrinterProfile = makePrinter({
  id: 'builtin-generic-printer',
  name: 'Generic 220 mm printer',
  source: 'builtin',
  vendor: 'Generic',
  model: 'FDM 220',
});

export const DEFAULT_PRINTERS: PrinterProfile[] = [DEFAULT_PRINTER];

/** Bed dimensions for the {@link PrintArea} config. */
export function printerBedConfig(printer: PrinterProfile): {
  printableAreaWidth: number;
  printableAreaHeight: number;
} {
  return {
    printableAreaWidth: printer.bedWidth,
    printableAreaHeight: printer.bedShape === 'circular' ? printer.bedWidth : printer.bedDepth,
  };
}

/** Hardware-owned slice parameters contributed by the active printer. */
export function printerSliceParams(printer: PrinterProfile): Partial<SlicingParams> {
  return {
    nozzle_diameter_mm: printer.nozzleDiameter,
    filament_diameter_mm: printer.filamentDiameter,
    gcode_flavor: printer.gcodeFlavor,
    travel_speed_mm_min: printer.maxTravelSpeed * 60,
    z_hop_mm: printer.zHop,
    retract_mm: printer.retractionLength,
    print_speed: printer.maxPrintSpeed,
  };
}
