import { Injector, runInInjectionContext, signal } from '@angular/core';
import { beforeEach, describe, expect, it } from 'vitest';
import { BrowserStorage } from './browser-storage';
import { SettingsDetailPreference } from './settings-detail-preference';

/**
 * Stand-in store, so the preference can be exercised without a DOM.
 *
 * Not a subclass: `BrowserStorage`'s constructor injects `DestroyRef` and
 * subscribes to `window`, neither of which exists here. This mirrors only the
 * two calls the preference actually makes.
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
  } as unknown as BrowserStorage;
}

describe('SettingsDetailPreference', () => {
  let storage: BrowserStorage;
  let injector: Injector;

  beforeEach(() => {
    storage = fakeStorage();
    injector = Injector.create({
      providers: [{ provide: BrowserStorage, useValue: storage }],
    });
  });

  const make = (): SettingsDetailPreference =>
    runInInjectionContext(injector, () => new SettingsDetailPreference());

  it('opens the panels at the calm view by default', () => {
    expect(make().mode()).toBe('everyday');
  });

  it('remembers a chosen floor', () => {
    const pref = make();
    pref.setMode('expert');
    expect(pref.mode()).toBe('expert');
    pref.setMode('advanced');
    expect(pref.mode()).toBe('advanced');
  });

  it('falls back to everyday for a value it does not recognise', () => {
    // A hand-edited or stale key must not leave the panel in an undefined state.
    storage.write('general.settingsDetail', 'wizard', 'local');
    expect(make().mode()).toBe('everyday');
  });
});
