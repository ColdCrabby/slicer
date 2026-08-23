import { uid } from './id';

/** Kinds of network connection a printer can use to receive prints (mocked). */
export type PrinterConnectionKind = 'none' | 'octoprint' | 'moonraker' | 'bambu' | 'prusalink';

export interface PrinterConnection {
  kind: PrinterConnectionKind;
  host?: string;
  connected: boolean;
}

export interface PrinterProfile {
  id: string;
  name: string;
  vendor: string;
  model: string;
  bedWidth: number;
  bedDepth: number;
  bedHeight: number;
  nozzleDiameter: number;
  connection: PrinterConnection;
}

export const PRINTER_CONNECTION_LABELS: Record<PrinterConnectionKind, string> = {
  none: 'Not connected',
  octoprint: 'OctoPrint',
  moonraker: 'Moonraker (Klipper)',
  bambu: 'Bambu Lab',
  prusalink: 'PrusaLink',
};

export function makePrinter(): PrinterProfile {
  return {
    id: uid(),
    name: 'New printer',
    vendor: 'Custom',
    model: 'Generic',
    bedWidth: 220,
    bedDepth: 220,
    bedHeight: 250,
    nozzleDiameter: 0.4,
    connection: { kind: 'none', connected: false },
  };
}

export const DEFAULT_PRINTERS: PrinterProfile[] = [
  {
    id: 'seed-mk4',
    name: 'Workshop MK4',
    vendor: 'Prusa',
    model: 'MK4',
    bedWidth: 250,
    bedDepth: 210,
    bedHeight: 220,
    nozzleDiameter: 0.4,
    connection: { kind: 'none', connected: false },
  },
  {
    id: 'seed-ender3',
    name: 'Garage Ender',
    vendor: 'Creality',
    model: 'Ender 3',
    bedWidth: 220,
    bedDepth: 220,
    bedHeight: 250,
    nozzleDiameter: 0.4,
    connection: { kind: 'none', connected: false },
  },
];
