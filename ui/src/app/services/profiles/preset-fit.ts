import type { FilamentProfile } from '../../models/filament.model';
import type { PrinterProfile } from '../../models/printer.model';
import type { ProcessProfile } from '../../models/print-profile.model';

/**
 * Whether a filament or process preset is a good fit for the machine it would
 * be printed on, and one plain sentence when it is not.
 *
 * This is the fourth kind of mismatch between profiles, and the only one with
 * nothing to merge. A preset asking for 300 mm/s on a machine commissioned at
 * 150 is not a value to reconcile — the process asks, the hardware does what it
 * can, and that is the right division. What is wrong is finding out afterwards.
 *
 * So the answer is a **word, not a setting**:
 *
 * - It never disables the option. A user who wants to drive a machine past what
 *   it was set up for is allowed to; plenty of printers are configured
 *   conservatively and run fine well above it.
 * - It never introduces a second copy of a print setting on the printer. Every
 *   comparison below reads a field the printer already carries for its own
 *   reasons — the velocity ceiling the firmware is given, the temperature
 *   limits read off its config, whether it has a chamber heater.
 * - It reports the **first** problem, not all of them. A list in a dropdown is
 *   not read; one clause is.
 */

/** A number out of a profile's sparse `params` bag, when it is a real one. */
function num(params: unknown, key: string): number | null {
  const value = (params as Record<string, unknown> | undefined)?.[key];
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

function bool(params: unknown, key: string): boolean {
  return (params as Record<string, unknown> | undefined)?.[key] === true;
}

/** Round for display — these are approximate limits, not measurements. */
function round(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

/**
 * Why this process preset is a stretch on `printer`, or `null` when it is not.
 *
 * Both comparisons are against limits the printer profile already holds:
 * `max_velocity` is the cap the slicer emits to the firmware and estimates
 * from, and `acceleration` is what the machine reported it is commissioned at.
 */
export function processFitWarning(process: ProcessProfile, printer: PrinterProfile): string | null {
  const machineVelocity = num(printer.params, 'max_velocity');
  const asksSpeed = num(process.params, 'print_speed');
  if (machineVelocity && asksSpeed && asksSpeed > machineVelocity) {
    return `Asks ${round(asksSpeed)} mm/s; ${printer.name} tops out at ${round(machineVelocity)}`;
  }

  const machineAccel = num(printer.params, 'acceleration');
  const asksAccel = num(process.params, 'acceleration');
  if (machineAccel && asksAccel && asksAccel > machineAccel) {
    return `Asks ${round(asksAccel)} mm/s²; ${printer.name} is set up for ${round(machineAccel)}`;
  }

  return null;
}

/**
 * Why this filament is a poor fit for `printer`, or `null` when it is not.
 *
 * Unlike a speed the machine simply will not reach, every case here **stalls**:
 * a heat target above what the hardware can do never arrives, so the print
 * waits on it indefinitely, and a chamber command a machine does not understand
 * is either ignored or fatal. Worth saying before the file is written.
 */
export function filamentFitWarning(
  filament: FilamentProfile,
  printer: PrinterProfile,
): string | null {
  const hotendLimit = num(printer.params, 'max_hotend_temp');
  const wantsNozzle = Math.max(
    num(filament.params, 'nozzle_temp') ?? 0,
    num(filament.params, 'nozzle_temp_first_layer') ?? 0,
  );
  if (hotendLimit && wantsNozzle > hotendLimit) {
    return `Needs ${round(wantsNozzle)} °C; this hotend is rated for ${round(hotendLimit)}`;
  }

  const bedLimit = num(printer.params, 'max_bed_temp');
  const wantsBed = Math.max(
    num(filament.params, 'bed_temp') ?? 0,
    num(filament.params, 'bed_temp_first_layer') ?? 0,
  );
  if (bedLimit && wantsBed > bedLimit) {
    return `Needs a ${round(wantsBed)} °C bed; this one reaches ${round(bedLimit)}`;
  }

  // Not a stall but a warp: the chamber target is simply never emitted, and a
  // chamber that never heats looks exactly like one that does until the part
  // lifts off the plate.
  const wantsChamber = num(filament.params, 'chamber_temp') ?? 0;
  if (wantsChamber > 0 && !bool(printer.params, 'heated_chamber')) {
    return `Wants a ${round(wantsChamber)} °C chamber; ${printer.name} has no chamber heater`;
  }

  return null;
}
