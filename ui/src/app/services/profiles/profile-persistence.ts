import type { Provider } from '@angular/core';
import { environment } from '../../../environments/environment';
import { resolveRuntimeMode } from '../../runtime/domain/runtime-mode.util';

/**
 * The four syncable profile categories. Mirrors the engine's `ProfileKind`
 * (`src/profiles/store.rs`) and the REST route token `/api/profiles/:kind`.
 *
 * Note the UI's "print profiles" are the engine's `processes` — the store keys
 * differ from the wire token, so callers pass the *engine* token here.
 */
export type ProfileCategory = 'printers' | 'filaments' | 'processes' | 'labels';

/** The whole engine-owned profile library, as returned by the engine. */
export interface ProfileLibrarySnapshot {
  printers?: unknown[];
  filaments?: unknown[];
  processes?: unknown[];
  labels?: unknown[];
}

/**
 * Persists the user-owned profile library *next to the engine* so it survives a
 * browser cache wipe. There is one implementation per runtime mode:
 *
 * - **native** → the Tauri host writes `profiles.toml` on the local machine.
 * - **cloud** → the slicer server writes `profiles.toml` beside `slicer.toml`.
 * - **web (wasm)** → the browser *is* the engine, so there is no separate
 *   store; the profile signals live in `localStorage` exactly as before and
 *   {@link isEngineBacked} is `false`.
 *
 * The profile stores treat `localStorage` as a fast local cache in every mode;
 * when {@link isEngineBacked} they additionally hydrate from, and write through
 * to, this backend.
 */
export abstract class ProfilePersistence {
  /** True when the library is persisted with the engine (native/cloud). */
  abstract readonly isEngineBacked: boolean;

  /** Shared load so the four stores don't each fetch the library on startup. */
  private pending: Promise<ProfileLibrarySnapshot> | null = null;

  /**
   * Pull the whole library from the engine. The four profile stores all call
   * this from their constructors, so the result is memoised: concurrent callers
   * share a single request instead of hitting the engine once per category.
   * Invalidated on {@link saveCategory} so a later load reflects fresh state.
   */
  loadLibrary(): Promise<ProfileLibrarySnapshot> {
    if (!this.isEngineBacked) {
      return Promise.resolve({});
    }
    if (!this.pending) {
      this.pending = this.fetchLibrary().catch((error) => {
        // Let a failed load be retried rather than caching the rejection.
        this.pending = null;
        throw error;
      });
    }
    return this.pending;
  }

  /** Replace one category in the engine store (whole-category write-through). */
  async saveCategory(category: ProfileCategory, items: unknown[]): Promise<void> {
    await this.persistCategory(category, items);
    // The library changed; drop the cache so the next load re-fetches.
    this.pending = null;
  }

  /**
   * Force a fresh whole-library fetch, bypassing the memoised load. Used when
   * the engine reports (over WebSocket) that a category changed elsewhere.
   */
  reloadLibrary(): Promise<ProfileLibrarySnapshot> {
    this.pending = null;
    return this.loadLibrary();
  }

  /** Backend-specific whole-library fetch. Only called when engine-backed. */
  protected abstract fetchLibrary(): Promise<ProfileLibrarySnapshot>;
  /** Backend-specific whole-category write. */
  protected abstract persistCategory(category: ProfileCategory, items: unknown[]): Promise<void>;
}

/** Browser-local backend (wasm): the store's own `localStorage` is the truth. */
export class BrowserProfilePersistence extends ProfilePersistence {
  readonly isEngineBacked = false;
  protected async fetchLibrary(): Promise<ProfileLibrarySnapshot> {
    return {};
  }
  protected async persistCategory(): Promise<void> {
    // No engine store — the profile store already wrote localStorage.
  }
}

/** Cloud backend: REST to the slicer server's `/api/profiles`. */
export class RemoteProfilePersistence extends ProfilePersistence {
  readonly isEngineBacked = true;
  private readonly base = environment.apiUrl;

  protected async fetchLibrary(): Promise<ProfileLibrarySnapshot> {
    const response = await fetch(`${this.base}/profiles`, {
      headers: { Accept: 'application/json' },
    });
    if (!response.ok) {
      throw new Error(`GET /profiles failed (${response.status})`);
    }
    return (await response.json()) as ProfileLibrarySnapshot;
  }

  protected async persistCategory(category: ProfileCategory, items: unknown[]): Promise<void> {
    const response = await fetch(`${this.base}/profiles/${category}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(items),
      // Survive a page-hide flush so a fast navigation doesn't drop the write.
      keepalive: true,
    });
    if (!response.ok) {
      throw new Error(`PUT /profiles/${category} failed (${response.status})`);
    }
  }
}

/** Native backend: Tauri `invoke` into the engine's on-disk store. */
export class NativeProfilePersistence extends ProfilePersistence {
  readonly isEngineBacked = true;

  protected async fetchLibrary(): Promise<ProfileLibrarySnapshot> {
    const { invoke } = await import('@tauri-apps/api/core');
    return (await invoke<ProfileLibrarySnapshot>('profiles_load')) ?? {};
  }

  protected async persistCategory(category: ProfileCategory, items: unknown[]): Promise<void> {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('profiles_save_category', { kind: category, items });
  }
}

/**
 * Bind {@link ProfilePersistence} to the backend for the active runtime mode.
 * Register once in the app providers.
 */
export function provideProfilePersistence(): Provider {
  return {
    provide: ProfilePersistence,
    useFactory: (): ProfilePersistence => {
      switch (resolveRuntimeMode()) {
        case 'native':
          return new NativeProfilePersistence();
        case 'cloud':
          return new RemoteProfilePersistence();
        default:
          return new BrowserProfilePersistence();
      }
    },
  };
}
