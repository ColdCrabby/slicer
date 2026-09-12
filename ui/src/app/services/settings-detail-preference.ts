import { computed, inject, Injectable } from '@angular/core';
import type { Tier } from '../schema-form/models/relevance';
import { BrowserStorage } from './browser-storage';

const SETTINGS_DETAIL_KEY = 'general.settingsDetail';

/** The tiers a user can ask every settings panel to open at. */
export type SettingsDetailMode = Tier;

/**
 * The level of detail the settings panels *open* at.
 *
 * Disclosure is otherwise per-section and per-intent, and deliberately so: the
 * tier model exists to keep a wall of 400 settings from being the first thing
 * anyone sees. But someone who already knows every parameter spends that model
 * pressing the same two controls on every section, every session, to get back
 * to where they always work.
 *
 * This is the floor, not a mode. It sets where a panel starts; the per-section
 * controls still reveal deeper from there, and none of it changes the shape of
 * the app, hides a capability, or gates anything. A user on `everyday` sees
 * exactly what they saw before.
 *
 * The options are labelled **Standard / Advanced / Everything**, and the first
 * of those matters: naming the default view "Simple" labels the reader rather
 * than the view, which is the one thing the tier model rules out — a
 * professional has to be able to work there without being handed the toy
 * version. "Everything" describes the result for the same reason "Expert"
 * would have described the person.
 */
@Injectable({ providedIn: 'root' })
export class SettingsDetailPreference {
  private readonly storage = inject(BrowserStorage);
  private readonly stored = this.storage.get(SETTINGS_DETAIL_KEY, 'local');

  /** The chosen floor; defaults to `everyday` — the calm view. */
  readonly mode = computed<SettingsDetailMode>(() => {
    const raw = this.stored();
    return raw === 'advanced' || raw === 'expert' ? raw : 'everyday';
  });

  setMode(mode: SettingsDetailMode): void {
    this.storage.write(SETTINGS_DETAIL_KEY, mode, 'local');
  }
}
