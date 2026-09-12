import { computed, inject, Injectable, signal } from '@angular/core';
import { BrowserStorage } from './browser-storage';
import { SlicerFile } from './slicer-file';

/**
 * Whether an edit re-slices the plate on its own.
 *
 * - `auto` — decide from how long the last slice took. A plate that comes back
 *   in a moment is re-sliced without being asked; one that costs real work
 *   waits for a deliberate press.
 * - `on` / `off` — force it either way, whatever the plate costs.
 */
export type AutoSliceMode = 'auto' | 'on' | 'off';

const AUTO_SLICE_KEY = 'general.autoSlice';

/**
 * How still the scene has to sit before an automatic re-slice fires.
 *
 * Long enough that dragging an object across the bed, or typing a three-digit
 * temperature, lands as one edit rather than a dozen; short enough that the
 * preview feels like it is keeping up rather than catching up. Every further
 * change restarts it, so the cost of a long edit is one slice, not one per
 * keystroke.
 */
export const AUTO_SLICE_DELAY_MS = 1200;

/**
 * The slice time at which `auto` stops re-slicing on its own.
 *
 * Below this the wait is shorter than the trip to the Slice button, so doing it
 * unasked is pure gain. Above it the machine would be busy for longer than the
 * user is likely to stay still, and an automatic slice starts costing more than
 * it saves — it competes with the next edit for the same cores. Five seconds is
 * roughly where a re-slice stops reading as "the preview updated" and starts
 * reading as "something is running".
 */
export const AUTO_SLICE_BUDGET_MS = 5000;

/**
 * Owns the app-wide policy for re-slicing after a change, and the measurements
 * that policy is made of.
 *
 * Some plates come back in under a second; some take a minute. The same
 * behaviour cannot be right for both, and asking the user to predict which they
 * have is asking the wrong person — the slicer already knows, because it just
 * timed one. `auto` therefore reads how long *this plate's* last slice took and
 * re-slices only while that stays cheap, which means a plate that grows heavy
 * stops auto-slicing by itself, and a plate that gets lighter starts again.
 *
 * The mode is a preference and is persisted. The timings are not — see
 * {@link measured}.
 *
 * Holds no reference to the slicing runtime: the timer that acts on this lives
 * in {@link Slicer}, which owns the drift signal and the slice call, so nothing
 * that merely wants to *read* the preference (the settings page) pulls the
 * runtime in behind it.
 */
@Injectable({ providedIn: 'root' })
export class AutoSlice {
  private readonly storage = inject(BrowserStorage);
  private readonly slicerFile = inject(SlicerFile);
  private readonly storedMode = this.storage.get(AUTO_SLICE_KEY, 'local');

  /**
   * Slice durations measured this session, keyed by workplate.
   *
   * In memory on purpose. The measurement belongs to the plate — one heavy
   * plate must not decide what a light one does — and equally to the machine
   * that produced it, so a figure written on a desktop would be wrong the
   * moment the same plate opened on a phone.
   *
   * Losing it on reload costs nothing, because an automatic re-slice can only
   * ever follow a slice the user pressed on that plate: the drift baseline it
   * waits on is set by slicing, so the timing is always taken before anything
   * needs to read it.
   */
  private readonly measured = signal<ReadonlyMap<string, number>>(new Map());

  /** The user's chosen mode; defaults to `auto`. */
  readonly mode = computed<AutoSliceMode>(() => {
    const raw = this.storedMode();
    return raw === 'on' || raw === 'off' || raw === 'auto' ? raw : 'auto';
  });

  /**
   * How long the open plate's last slice took, in milliseconds, or `null`
   * before it has been sliced in this session.
   */
  readonly lastSliceMs = computed<number | null>(() => {
    const uuid = this.slicerFile.requestUuid();
    return uuid === null ? null : (this.measured().get(uuid) ?? null);
  });

  /**
   * Whether a change to the open plate should currently re-slice on its own.
   *
   * With nothing timed for it yet this reads `true`, which costs nothing: an
   * automatic re-slice only ever follows a slice the user asked for, so by the
   * time it could fire there is always a real measurement to have replaced this
   * guess.
   */
  readonly enabled = computed<boolean>(() => {
    switch (this.mode()) {
      case 'on':
        return true;
      case 'off':
        return false;
      default: {
        const last = this.lastSliceMs();
        return last === null || last <= AUTO_SLICE_BUDGET_MS;
      }
    }
  });

  /**
   * True while a re-slice is queued behind {@link AUTO_SLICE_DELAY_MS}, so the
   * slice card can say so instead of nagging for a press that is already
   * coming. Written by {@link Slicer}, which owns the timer.
   */
  readonly pending = signal(false);

  setMode(mode: AutoSliceMode): void {
    this.storage.write(AUTO_SLICE_KEY, mode, 'local');
  }

  /**
   * Step to the next mode — Auto → On → Off → Auto.
   *
   * Three states on one button is what keeps `auto` reachable from the toolbar:
   * a plain switch would strand anyone who tried it once in a permanent manual
   * or permanent automatic setting they could only undo from Settings.
   */
  cycleMode(): void {
    const next: Record<AutoSliceMode, AutoSliceMode> = { auto: 'on', on: 'off', off: 'auto' };
    this.setMode(next[this.mode()]);
  }

  /**
   * Remember what the slice that just finished cost, against the plate it was
   * run for. Ignores unmeasured runs and a slice with no plate behind it.
   */
  recordSliceDuration(ms: number | null): void {
    const uuid = this.slicerFile.requestUuid();
    if (uuid === null || ms === null || !Number.isFinite(ms) || ms <= 0) {
      return;
    }
    this.measured.update((byPlate) => new Map(byPlate).set(uuid, Math.round(ms)));
  }
}
