import { Injectable, computed, inject, signal } from '@angular/core';
import type { SettingContractId } from '../models/setting-contract';
import { BrowserStorage } from './browser-storage';
import { WorkplatePersistence, type WorkplateSetup } from './workplate-persistence';

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
  /**
   * Where each object sat, and which uploaded file it came from — the plate's
   * "what and where". Recorded so reopening a plate restores the arrangement;
   * the slice request carries its own copy of the live scene regardless, so a
   * stale record here can never change what gets printed.
   */
  objects: WorkplateSetup['objects'];
}

const EMPTY: WorkplateSettings = Object.freeze({ overrides: {}, presets: {}, objects: [] });

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
 * **Persisted where the engine runs**, not only in this browser — the same rule
 * the profile library follows, and for the same reason: a cloud user who clears
 * their browser must not lose their plates. `localStorage` stays the fast local
 * cache and is the *only* copy in the web build, where the browser is the
 * engine. See {@link WorkplatePersistence}.
 *
 * Writes are debounced, which is why {@link status} has a real `pending` state
 * to report rather than a decorative one.
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

  private readonly persistence = inject(WorkplatePersistence);

  /** Plates already pulled from the engine, so each is fetched at most once. */
  private readonly hydrated = new Set<string>();

  private debounce: ReturnType<typeof setTimeout> | null = null;
  private settle: ReturnType<typeof setTimeout> | null = null;
  /** Whether the pending write has something to report to the user. */
  private announce = false;
  /** Plates changed since the last write, so only those are sent up. */
  private readonly touched = new Set<string>();

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
   * Record where the objects sit. Silent — the user is dragging a model, and
   * they can see it move; a "Saved" line for that is noise.
   */
  setObjects(uuid: string | null | undefined, objects: WorkplateSettings['objects']): void {
    if (JSON.stringify(this.settingsFor(uuid).objects ?? []) === JSON.stringify(objects ?? [])) {
      return;
    }
    this.#update(uuid, (plate) => ({ ...plate, objects }), false);
  }

  /**
   * Pull a plate's setup from the engine, once, and adopt it locally.
   *
   * The engine's copy wins on a cold open: it is the one that survived the
   * browser being cleared, and it is what another device would have written.
   * A plate this browser has already loaded is left alone — re-adopting
   * mid-session would fight the user's live edits.
   *
   * Failure is not fatal: the local cache is still there, and a plate that
   * opens with the settings this browser remembers beats one that refuses to
   * open at all.
   */
  async hydrate(uuid: string | null | undefined): Promise<void> {
    const key = this.#key(uuid);
    if (!this.persistence.isEngineBacked || key === DRAFT_KEY || this.hydrated.has(key)) {
      return;
    }
    this.hydrated.add(key);
    try {
      // "Nothing saved" arrives two ways: the REST route answers with an empty
      // document rather than a 404, the Tauri command answers with null.
      const remote = await this.persistence.load(key);
      const adopted: WorkplateSettings = {
        overrides: (remote?.overrides as Record<string, unknown>) ?? {},
        presets: {
          printer: remote?.presets?.printer ?? undefined,
          filament: remote?.presets?.filament ?? undefined,
          process: remote?.presets?.process ?? undefined,
        },
        objects: remote?.objects ?? [],
      };
      if (this.#isEmpty(adopted)) {
        // The engine has nothing for this plate; whatever is cached locally is
        // the only record, so push it up rather than blanking it.
        void this.#persistRemote(key);
        return;
      }
      this.plates.update((plates) => ({ ...plates, [key]: adopted }));
      this.storage.writeJson(WORKPLATE_SETTINGS_STORAGE_KEY, this.plates(), 'local');
    } catch (error) {
      console.warn(`[workplate] could not load '${key}' from the engine; using local cache`, error);
    }
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
    this.touched.add(key);
    this.#schedule(announce);
  }

  /** Send one plate's document to the engine. Never throws at the caller. */
  async #persistRemote(key: string): Promise<void> {
    if (!this.persistence.isEngineBacked || key === DRAFT_KEY) {
      return;
    }
    const plate = this.plates()[key];
    if (!plate) {
      return;
    }
    try {
      await this.persistence.save(key, {
        presets: plate.presets,
        overrides: plate.overrides,
        objects: plate.objects ?? [],
      });
    } catch (error) {
      // Local storage already holds it; surface the failure without losing it.
      this._error.set(error instanceof Error ? error.message : String(error));
      this._status.set('error');
    }
  }

  /** Whether a document carries anything the defaults would not supply. */
  #isEmpty(plate: WorkplateSettings): boolean {
    return (
      Object.keys(plate.overrides ?? {}).length === 0 &&
      !plate.presets?.printer &&
      !plate.presets?.filament &&
      !plate.presets?.process &&
      (plate.objects?.length ?? 0) === 0
    );
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
      // Write through to the engine for every plate this pass touched. The
      // local cache is already written above, so a failed upload degrades to
      // "this browser remembers it" rather than losing the edit.
      for (const key of this.touched) {
        void this.#persistRemote(key);
      }
      this.touched.clear();
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
