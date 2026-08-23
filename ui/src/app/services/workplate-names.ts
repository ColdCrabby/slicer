import { Injectable, inject, signal } from '@angular/core';
import { BrowserStorage } from './browser-storage';

const STORAGE_KEY = 'workplate.names';

/**
 * Remembers the user-chosen display name for each workplate, keyed by its
 * `request_uuid`. Backend scenes are ephemeral per WS connection, so — like the
 * printer and filament profiles — names live in localStorage and survive
 * reloads. Editing a name here is the single source of truth the scene title
 * and history list both read from.
 */
@Injectable({ providedIn: 'root' })
export class WorkplateNames {
  private readonly storage = inject(BrowserStorage);

  private readonly _names = signal<Record<string, string>>(
    this.storage.getJson<Record<string, string>>(STORAGE_KEY, 'local') ?? {},
  );

  /** Reactive map of `request_uuid` → custom name. */
  readonly names = this._names.asReadonly();

  /** The custom name for a workplate, or `null` if it was never renamed. */
  nameFor(uuid: string | null | undefined): string | null {
    if (!uuid) {
      return null;
    }
    return this._names()[uuid] ?? null;
  }

  /** Store (or, when blank, clear) the custom name for a workplate. */
  setName(uuid: string, name: string): void {
    const trimmed = name.trim();
    this._names.update((map) => {
      const next = { ...map };
      if (trimmed) {
        next[uuid] = trimmed;
      } else {
        delete next[uuid];
      }
      return next;
    });
    this.storage.writeJson(STORAGE_KEY, this._names(), 'local');
  }
}
