import { TestBed } from '@angular/core/testing';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { WORKPLATE_SAVE_DEBOUNCE_MS, WorkplateSettingsStore } from './workplate-settings';

const PLATE_A = '11111111-1111-1111-1111-111111111111';
const PLATE_B = '22222222-2222-2222-2222-222222222222';

function store(): WorkplateSettingsStore {
  return TestBed.inject(WorkplateSettingsStore);
}

describe('WorkplateSettingsStore', () => {
  beforeEach(() => {
    localStorage.clear();
    TestBed.resetTestingModule();
  });

  it("keeps each plate's diff to itself", () => {
    const plates = store();
    plates.setOverrides(PLATE_A, { layer_height: 0.12 });
    plates.setOverrides(PLATE_B, { nozzle_temp: 240 });

    expect(plates.settingsFor(PLATE_A).overrides).toEqual({ layer_height: 0.12 });
    expect(plates.settingsFor(PLATE_B).overrides).toEqual({ nozzle_temp: 240 });
  });

  it('reads an untouched plate as inheriting everything', () => {
    expect(store().settingsFor('never-opened')).toEqual({ overrides: {}, presets: {} });
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
});
