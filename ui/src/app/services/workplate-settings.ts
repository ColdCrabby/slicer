import { Injectable, computed, inject, signal } from '@angular/core';
import type { SettingContractId } from '../models/setting-contract';
import { BrowserStorage } from './browser-storage';

/** Where the per-plate diffs live. Exported so the Danger Zone can wipe it. */
export const WORKPLATE_SETTINGS_STORAGE_KEY = 'workplate.settings';

/** Key the not-yet-uploaded plate parks its settings under. */
const DRAFT_KEY = '__draft__';

/** How long the "Saved" confirmation stays up before retiring itself. */
const SAVED_LINGER_MS = 1200;

/** How long to coalesce rapid edits (dragging a slider) into one write. */
export const WORKPLATE_SAVE_DEBOUNCE_MS = 400;

/** Lifecycle of the debounced write. `pending` is real: nothing is stored yet. */
export type WorkplateSaveStatus = 'idle' | 'pending' | 'saving' | 'saved' | 'error';

/** Which preset was chosen per contract when the plate was last touched. */
export type PlatePresetIds = Partial<Record<SettingContractId, string>>;

/** One plate's remembered slice setup. */
export interface WorkplateSettings {
  /**
   * Sparse deviation from the resolved preset stack — the same shape the engine
   * takes as `ProfileSelection.overrides`. Only keys the user actually changed.
   */
  overrides: Record<string, unknown>;
  /** Presets the overrides are measured against. */
  presets: PlatePresetIds;
}

const EMPTY: WorkplateSettings = Object.freeze({ overrides: {}, presets: {} });

/**
 * Remembers each workplate's slice setup: which printer / filament / process it
 * was set up with, and the sparse diff the user applied on top.
 *
 * **Only the diff is stored.** A value equal to the resolved preset stack is
 * not an override, it is an inheritance, and storing it would freeze the plate
 * against the profile it came from — editing the process profile afterwards
 * would then change every plate except the ones that had been opened. The same
 * sparse object is what goes on the wire at slice time, so what is remembered
 * and what is sent cannot drift.
 *
 * The presets ride along because a diff means nothing without the baseline it
 * was measured against: reopening a plate that was set up for PETG must bring
 * PETG back, not reinterpret its `-15 °C` against whatever is selected now.
 *
 * Storage matches {@link WorkplateNames} — server scenes are ephemeral per WS
 * connection, so plate-scoped UI state lives in `localStorage` and survives
 * reloads. Writes are debounced, which is why {@link status} has a real
 * `pending` state to report rather than a decorative one.
 */
@Injectable({ providedIn: 'root' })
export class WorkplateSettingsStore {
  private readonly storage = inject(BrowserStorage);

  private readonly plates = signal<Record<string, WorkplateSettings>>(
    this.storage.getJson<Record<string, WorkplateSettings>>(
      WORKPLATE_SETTINGS_STORAGE_KEY,
      'local',
    ) ?? {},
  );

  private readonly _status = signal<WorkplateSaveStatus>('idle');
  /** Save lifecycle, for the panel's indicator. `idle` renders nothing. */
  readonly status = this._status.asReadonly();

  private readonly _error = signal<string | null>(null);
  /** Why the last write failed (a full storage quota, typically). */
  readonly error = this._error.asReadonly();

  private debounce: ReturnType<typeof setTimeout> | null = null;
  private settle: ReturnType<typeof setTimeout> | null = null;
  /** Whether the pending write has something to report to the user. */
  private announce = false;

  constructor() {
    // A pending write must not die with the tab. Both events fire before the
    // page is torn down, and the write itself is synchronous, so flushing here
    // is enough — there is no request in flight to keep alive.
    const flushIfHidden = () => {
      if (document.visibilityState === 'hidden') {
        this.flush();
      }
    };
    document.addEventListener('visibilitychange', flushIfHidden);
    window.addEventListener('pagehide', () => this.flush());
  }

  /** Everything remembered for `uuid`; an untouched plate reads as empty. */
  settingsFor(uuid: string | null | undefined): WorkplateSettings {
    return this.plates()[this.#key(uuid)] ?? EMPTY;
  }

  /** Reactive view of one plate's override diff. */
  overridesFor(uuid: string | null | undefined) {
    return computed(() => this.settingsFor(uuid).overrides);
  }

  /**
   * Replace the plate's override diff wholesale.
   *
   * Callers hand over the finished diff rather than a patch: deciding whether a
   * value is an override needs the resolved baseline, which lives with
   * {@link Slicer}, not here.
   */
  setOverrides(uuid: string | null | undefined, overrides: Record<string, unknown>): void {
    this.#update(uuid, (current) => ({ ...current, overrides }));
  }

  /**
   * Record which presets this plate is set up with.
   *
   * Written silently. Binding a plate to the selection happens on load, with no
   * user involved, and an indicator that announces "Saved" for something nobody
   * did is noise at best — it teaches the user to ignore the one case that
   * matters, which is their own settings failing to persist.
   */
  setPresets(uuid: string | null | undefined, presets: PlatePresetIds): void {
    const current = this.settingsFor(uuid).presets;
    if (
      current.printer === presets.printer &&
      current.filament === presets.filament &&
      current.process === presets.process
    ) {
      return;
    }
    this.#update(uuid, (plate) => ({ ...plate, presets }), false);
  }

  /**
   * Move the draft plate's settings onto the uuid the engine just assigned.
   *
   * A plate is configured before it is uploaded — settings changed on the drop
   * screen belong to the plate that upload produces, not to the next one.
   */
  adoptDraft(uuid: string): void {
    const draft = this.plates()[DRAFT_KEY];
    if (!draft || uuid === DRAFT_KEY) {
      return;
    }
    this.plates.update((plates) => {
      const next = { ...plates, [uuid]: draft };
      delete next[DRAFT_KEY];
      return next;
    });
    // Silent: moving the diff onto its plate is bookkeeping. The settings it
    // carries were already confirmed when the user made them.
    this.#schedule(false);
  }

  /** Write any pending change now, bypassing the debounce. */
  flush(): void {
    if (this.debounce === null) {
      return;
    }
    clearTimeout(this.debounce);
    this.debounce = null;
    this.#persist();
  }

  #update(
    uuid: string | null | undefined,
    change: (current: WorkplateSettings) => WorkplateSettings,
    announce = true,
  ): void {
    const key = this.#key(uuid);
    this.plates.update((plates) => ({ ...plates, [key]: change(plates[key] ?? EMPTY) }));
    this.#schedule(announce);
  }

  #schedule(announce: boolean): void {
    this.announce ||= announce;
    if (this.announce) {
      this._status.set('pending');
    }
    if (this.debounce !== null) {
      clearTimeout(this.debounce);
    }
    this.debounce = setTimeout(() => {
      this.debounce = null;
      this.#persist();
    }, WORKPLATE_SAVE_DEBOUNCE_MS);
  }

  #persist(): void {
    const announce = this.announce;
    this.announce = false;
    if (announce) {
      this._status.set('saving');
    }
    try {
      this.storage.writeJson(WORKPLATE_SETTINGS_STORAGE_KEY, this.plates(), 'local');
      this._error.set(null);
      if (!announce) {
        return;
      }
      this._status.set('saved');
      if (this.settle !== null) {
        clearTimeout(this.settle);
      }
      this.settle = setTimeout(() => {
        this.settle = null;
        if (this._status() === 'saved') {
          this._status.set('idle');
        }
      }, SAVED_LINGER_MS);
    } catch (error: unknown) {
      // Almost always a full quota. Say so instead of pretending it saved —
      // the user is one reload away from losing the settings they just tuned.
      // Reported whether or not the write announced itself: a plate that cannot
      // be stored is worth saying out loud even when nobody asked.
      this._error.set(error instanceof Error ? error.message : String(error));
      this._status.set('error');
    }
  }

  /** Plates are keyed by `request_uuid`; a plate without one is the draft. */
  #key(uuid: string | null | undefined): string {
    return uuid ?? DRAFT_KEY;
  }
}
