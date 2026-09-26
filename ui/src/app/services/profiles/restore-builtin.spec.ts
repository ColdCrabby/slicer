import { TestBed } from '@angular/core/testing';
import { beforeEach, describe, expect, it } from 'vitest';
import { ProfilePersistence, type ProfileCategory } from './profile-persistence';
import { PrintersStore } from './printers-store';
import { DEFAULT_PRINTER } from '../../models/printer.model';

/** Local-only stand-in — no engine to write through to. */
class FakeProfilePersistence extends ProfilePersistence {
  readonly isEngineBacked = false;
  protected fetchLibrary() {
    return Promise.resolve({});
  }
  protected persistCategory(_category: ProfileCategory, _items: unknown[]) {
    return Promise.resolve();
  }
  exportLibrary(): never {
    throw new Error('not used in this test');
  }
}

// A built-in is edited in place — that is the point of the editor saying so —
// and "Restore defaults" is the safety net that makes editing one free.
describe('restoring a built-in', () => {
  let store: PrintersStore;

  beforeEach(() => {
    localStorage.clear();
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [{ provide: ProfilePersistence, useClass: FakeProfilePersistence }],
    });
    store = TestBed.inject(PrintersStore);
  });

  it('puts the shipped name and values back after edits', () => {
    const id = DEFAULT_PRINTER.id;
    store.update(id, { name: 'My bedslinger', bed_width: 300 });
    store.restoreBuiltin(id);
    const restored = store.getById(id)!;
    expect(restored.name).toBe(DEFAULT_PRINTER.name);
    expect(restored.bed_width).toBe(DEFAULT_PRINTER.bed_width);
  });

  it('keeps the labels the user filed it under', () => {
    const id = DEFAULT_PRINTER.id;
    store.update(id, { label_ids: ['favourite'], bed_width: 300 });
    store.restoreBuiltin(id);
    expect(store.getById(id)!.label_ids).toEqual(['favourite']);
  });

  it('never shares the seed object, so a later edit cannot change the defaults', () => {
    const id = DEFAULT_PRINTER.id;
    store.restoreBuiltin(id);
    const params = store.getById(id)!.params as Record<string, unknown>;
    params['nozzle_diameter_mm'] = 1;
    const shipped = DEFAULT_PRINTER.params as Record<string, unknown>;
    expect(shipped['nozzle_diameter_mm']).toBe(0.4);
  });

  it('offers restore for a built-in only', () => {
    const copy = store.duplicate(DEFAULT_PRINTER.id)!;
    expect(store.canRestore(DEFAULT_PRINTER.id)).toBe(true);
    expect(store.canRestore(copy.id)).toBe(false);
    store.restoreBuiltin(copy.id);
    expect(store.getById(copy.id)!.name).toBe(copy.name);
  });

  it('records an edit made here, which a reload does not', () => {
    expect(store.lastEdit()).toBeNull();
    store.update(DEFAULT_PRINTER.id, { bed_width: 250 });
    expect(store.lastEdit()?.id).toBe(DEFAULT_PRINTER.id);
  });
});
