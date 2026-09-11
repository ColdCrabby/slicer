import { TestBed } from '@angular/core/testing';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { WORKPLATE_SAVE_DEBOUNCE_MS, WorkplateSettingsStore } from './workplate-settings';
import { WorkplatePersistence, type WorkplateSetup } from './workplate-persistence';

const PLATE_A = '11111111-1111-1111-1111-111111111111';
const PLATE_B = '22222222-2222-2222-2222-222222222222';

/** Stand-in engine store, so a test can see what would have been sent up. */
class FakePersistence extends WorkplatePersistence {
  isEngineBacked = true;
  readonly saved = new Map<string, WorkplateSetup>();
  readonly remote = new Map<string, WorkplateSetup>();
  failNextSave = false;

  async load(requestUuid: string): Promise<WorkplateSetup | null> {
    return this.remote.get(requestUuid) ?? null;
  }

  async save(requestUuid: string, setup: WorkplateSetup): Promise<void> {
    if (this.failNextSave) {
      this.failNextSave = false;
      throw new Error('engine unreachable');
    }
    this.saved.set(requestUuid, setup);
  }
}

let engine: FakePersistence;

function store(): WorkplateSettingsStore {
  return TestBed.inject(WorkplateSettingsStore);
}

describe('WorkplateSettingsStore', () => {
  beforeEach(() => {
    localStorage.clear();
    TestBed.resetTestingModule();
    engine = new FakePersistence();
    TestBed.configureTestingModule({
      providers: [{ provide: WorkplatePersistence, useValue: engine }],
    });
  });

  it("keeps each plate's diff to itself", () => {
    const plates = store();
    plates.setOverrides(PLATE_A, { layer_height: 0.12 });
    plates.setOverrides(PLATE_B, { nozzle_temp: 240 });

    expect(plates.settingsFor(PLATE_A).overrides).toEqual({ layer_height: 0.12 });
    expect(plates.settingsFor(PLATE_B).overrides).toEqual({ nozzle_temp: 240 });
  });

  it('reads an untouched plate as inheriting everything', () => {
    expect(store().settingsFor('never-opened')).toEqual({
      overrides: {},
      presets: {},
      objects: [],
    });
  });

  it('remembers the presets the diff was measured against', () => {
    const plates = store();
    plates.setPresets(PLATE_A, { printer: 'p1', filament: 'petg', process: 'fine' });
    expect(plates.settingsFor(PLATE_A).presets.filament).toBe('petg');
  });

  it('survives a reload', () => {
    const plates = store();
    plates.setOverrides(PLATE_A, { layer_height: 0.12 });
    plates.setPresets(PLATE_A, { filament: 'petg' });
    plates.flush();

    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [{ provide: WorkplatePersistence, useValue: engine }],
    });
    const reloaded = store();
    expect(reloaded.settingsFor(PLATE_A).overrides).toEqual({ layer_height: 0.12 });
    expect(reloaded.settingsFor(PLATE_A).presets.filament).toBe('petg');
  });

  it('carries settings tuned on the empty plate onto the plate they produced', () => {
    const plates = store();
    plates.setOverrides(null, { infill_density: 0.35 });
    plates.adoptDraft(PLATE_A);

    expect(plates.settingsFor(PLATE_A).overrides).toEqual({ infill_density: 0.35 });
    expect(plates.settingsFor(null).overrides).toEqual({});
  });

  it('coalesces a burst of edits into one write', () => {
    vi.useFakeTimers();
    try {
      const plates = store();
      const written = vi.spyOn(Storage.prototype, 'setItem');

      plates.setOverrides(PLATE_A, { layer_height: 0.12 });
      plates.setOverrides(PLATE_A, { layer_height: 0.14 });
      plates.setOverrides(PLATE_A, { layer_height: 0.16 });
      expect(plates.status()).toBe('pending');
      expect(written.mock.calls.length).toBe(0);

      vi.advanceTimersByTime(WORKPLATE_SAVE_DEBOUNCE_MS);
      expect(written.mock.calls.length).toBe(1);
      expect(plates.status()).toBe('saved');
    } finally {
      vi.restoreAllMocks();
      vi.useRealTimers();
    }
  });

  it('stores the preset binding without announcing a save nobody asked for', () => {
    vi.useFakeTimers();
    try {
      const plates = store();
      plates.setPresets(PLATE_A, { filament: 'petg' });
      expect(plates.status()).toBe('idle');

      vi.advanceTimersByTime(WORKPLATE_SAVE_DEBOUNCE_MS);
      expect(plates.status()).toBe('idle');
      expect(plates.settingsFor(PLATE_A).presets.filament).toBe('petg');
    } finally {
      vi.useRealTimers();
    }
  });

  it('still announces the edit when a silent write is coalesced with it', () => {
    vi.useFakeTimers();
    try {
      const plates = store();
      plates.setPresets(PLATE_A, { filament: 'petg' });
      plates.setOverrides(PLATE_A, { nozzle_temp: 245 });
      expect(plates.status()).toBe('pending');

      vi.advanceTimersByTime(WORKPLATE_SAVE_DEBOUNCE_MS);
      expect(plates.status()).toBe('saved');
    } finally {
      vi.useRealTimers();
    }
  });

  it('reports a write it could not make instead of claiming it saved', () => {
    vi.useFakeTimers();
    try {
      const plates = store();
      vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
        throw new DOMException('quota', 'QuotaExceededError');
      });

      plates.setOverrides(PLATE_A, { layer_height: 0.12 });
      vi.advanceTimersByTime(WORKPLATE_SAVE_DEBOUNCE_MS);

      expect(plates.status()).toBe('error');
      expect(plates.error()).toContain('quota');
    } finally {
      vi.restoreAllMocks();
      vi.useRealTimers();
    }
  });

  it('sends the plate up to the engine, as references rather than copies', async () => {
    const plates = store();
    plates.setOverrides(PLATE_A, { layer_height: 0.12 });
    plates.setPresets(PLATE_A, { printer: 'p1', filament: 'petg', process: 'fine' });
    plates.flush();
    await Promise.resolve();

    const sent = engine.saved.get(PLATE_A);
    expect(sent?.presets).toEqual({ printer: 'p1', filament: 'petg', process: 'fine' });
    expect(sent?.overrides).toEqual({ layer_height: 0.12 });
    expect(JSON.stringify(sent).includes('start_gcode')).toBe(false);
  });

  it('keeps the edit locally when the engine refuses it', async () => {
    const plates = store();
    engine.failNextSave = true;
    plates.setOverrides(PLATE_A, { layer_height: 0.12 });
    plates.flush();
    await Promise.resolve();
    await Promise.resolve();

    expect(plates.settingsFor(PLATE_A).overrides).toEqual({ layer_height: 0.12 });
    expect(plates.status()).toBe('error');
  });

  it('adopts the engine copy on a cold open', async () => {
    engine.remote.set(PLATE_A, {
      presets: { filament: 'builtin-generic-petg' },
      overrides: { layer_height: 0.3 },
      objects: [],
    });
    const plates = store();
    await plates.hydrate(PLATE_A);

    expect(plates.settingsFor(PLATE_A).overrides).toEqual({ layer_height: 0.3 });
    expect(plates.settingsFor(PLATE_A).presets.filament).toBe('builtin-generic-petg');
  });

  it('pushes this browser up when the engine has nothing for the plate', async () => {
    const plates = store();
    plates.setOverrides(PLATE_A, { layer_height: 0.12 });
    plates.flush();
    engine.saved.clear();

    await plates.hydrate(PLATE_A);
    await Promise.resolve();

    expect(plates.settingsFor(PLATE_A).overrides).toEqual({ layer_height: 0.12 });
    expect(engine.saved.get(PLATE_A)?.overrides).toEqual({ layer_height: 0.12 });
  });

  it('fetches a plate from the engine at most once', async () => {
    const plates = store();
    const spy = vi.spyOn(engine, 'load');
    await plates.hydrate(PLATE_A);
    await plates.hydrate(PLATE_A);
    expect(spy.mock.calls.length).toBe(1);
  });

  it('never sends the unsaved draft plate anywhere', async () => {
    const plates = store();
    plates.setOverrides(null, { layer_height: 0.12 });
    plates.flush();
    await Promise.resolve();

    expect(engine.saved.size).toBe(0);
    expect(plates.settingsFor(null).overrides).toEqual({ layer_height: 0.12 });
  });
});
