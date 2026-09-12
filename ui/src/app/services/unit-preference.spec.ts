import { Injector, runInInjectionContext, signal } from '@angular/core';
import { beforeEach, describe, expect, it } from 'vitest';
import { BrowserStorage } from './browser-storage';
import { UnitPreference } from './unit-preference';

/**
 * Stand-in store, so the preference can be exercised without a DOM.
 *
 * Not a subclass: `BrowserStorage`'s constructor injects `DestroyRef` and
 * subscribes to `window`, neither of which exists here. This mirrors only the
 * three calls the preference actually makes.
 */
function fakeStorage() {
  const values = new Map<string, ReturnType<typeof signal<string | null>>>();
  const get = (key: string) => {
    let existing = values.get(key);
    if (!existing) {
      existing = signal<string | null>(null);
      values.set(key, existing);
    }
    return existing;
  };
  return {
    get,
    write: (key: string, value: string) => get(key).set(value),
    writeJson: (key: string, value: unknown) => get(key).set(JSON.stringify(value)),
  } as unknown as BrowserStorage;
}

describe('UnitPreference', () => {
  let storage: BrowserStorage;
  let injector: Injector;

  beforeEach(() => {
    storage = fakeStorage();
    injector = Injector.create({ providers: [{ provide: BrowserStorage, useValue: storage }] });
  });

  const make = (): UnitPreference => runInInjectionContext(injector, () => new UnitPreference());

  it('expresses no preference until the user presses a unit', () => {
    // Absent means "the family's own default", which is mm/s for speed.
    expect(make().display()).toEqual({});
  });

  it('remembers a chosen unit', () => {
    const pref = make();
    pref.set('speed', 'mm_min');
    expect(pref.display()).toEqual({ speed: 'mm_min' });
  });

  it('ignores a unit the family does not offer', () => {
    const pref = make();
    pref.set('speed', 'furlongs_per_fortnight');
    expect(pref.display()).toEqual({});
  });

  it('moves off the family default on the first press', () => {
    // Nothing is stored yet, so the toggle has to resolve what is *shown*
    // before advancing — otherwise the first press picks mm/s again and the
    // control looks broken.
    const pref = make();
    pref.cycle('speed');
    expect(pref.display()).toEqual({ speed: 'mm_min' });
  });

  it('wraps back round', () => {
    const pref = make();
    pref.cycle('speed');
    pref.cycle('speed');
    expect(pref.display()).toEqual({ speed: 'mm_s' });
  });

  it('drops a stored value it cannot use', () => {
    // A hand-edited key, or one naming a unit a later release stopped offering.
    storage.write('general.unitDisplay', '{"speed":"parsecs","nonsense":1}', 'local');
    expect(make().display()).toEqual({});
  });

  it('survives a corrupt entry', () => {
    storage.write('general.unitDisplay', 'not json', 'local');
    expect(make().display()).toEqual({});
  });
});
