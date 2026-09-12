import { Injectable, inject, signal } from '@angular/core';
import { BrowserStorage } from './browser-storage';

const COLLAPSED_KEY = 'settings-nav.collapsed';

/**
 * Whether the Settings section list is folded down to an icon rail.
 *
 * A service rather than shell state because two unrelated surfaces depend on
 * the answer: the shell draws the rail, and the profile editors show their
 * contents outline **only** while it is folded. Settings is already three
 * columns wide — sections, list, editor — and a fourth is a column too many;
 * folding the first is what buys the room for the last.
 */
@Injectable({ providedIn: 'root' })
export class SettingsNav {
  private readonly storage = inject(BrowserStorage);

  readonly collapsed = signal(this.storage.getJson<boolean>(COLLAPSED_KEY, 'local') ?? false);

  toggle(): void {
    const next = !this.collapsed();
    this.collapsed.set(next);
    this.storage.writeJson(COLLAPSED_KEY, next, 'local');
  }
}
