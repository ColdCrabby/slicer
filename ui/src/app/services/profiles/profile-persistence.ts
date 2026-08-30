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
 * Shape of a profile export. Mirrors the engine's `ProfileExportFormat`.
 *
 * - `bundle` — a ZIP with one TOML file per profile.
 * - `toml` — a single `profiles.toml`, the file the engine and CLI read.
 */
export type ProfileExportFormat = 'bundle' | 'toml';

/** A rendered export, ready to be saved. */
export interface ProfileExportArtifact {
  filename: string;
  mime: string;
  bytes: Uint8Array;
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

  /**
   * Render the profile library as TOML for download.
   *
   * The engine does the rendering in every runtime, so the artifact is exactly
   * what the CLI reads — there is no TypeScript TOML writer to drift.
   *
   * `local` is the UI's own copy of the library. Engine-backed runtimes ignore
   * it and export what is actually persisted next to the engine (the source of
   * truth the CLI would read); the web runtime, where the browser *is* the
   * engine, has nothing else to export, so it hands this to the WASM exporter.
   */
  abstract exportLibrary(
    format: ProfileExportFormat,
    local: ProfileLibrarySnapshot,
  ): Promise<ProfileExportArtifact>;

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

  /**
   * Export through the WASM engine: in this runtime the browser *is* the
   * engine, so the library the UI holds is the library, and the same Rust
   * renderer the server and desktop use produces the bytes.
   *
   * The binding ships with the `web-slicer` WASM build — the one this runtime
   * always loads. It is looked up dynamically because the cloud/native bundles
   * are generated from a WASM build without the profile bindings (they export
   * through REST / Tauri instead), and their type declarations therefore do not
   * declare it.
   */
  async exportLibrary(
    format: ProfileExportFormat,
    local: ProfileLibrarySnapshot,
  ): Promise<ProfileExportArtifact> {
    const wasm = (await import('../../../generated/scene-wasm/scene_engine')) as unknown as {
      default: (options: { module_or_path: string }) => Promise<unknown>;
      exportProfileLibrary?: (
        library: ProfileLibrarySnapshot,
        format: ProfileExportFormat,
      ) => ProfileExportArtifact;
    };
    await wasm.default({ module_or_path: 'scene_engine_bg.wasm' });
    if (!wasm.exportProfileLibrary) {
      throw new Error('This build cannot export profiles');
    }
    return wasm.exportProfileLibrary(local, format);
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

  async exportLibrary(format: ProfileExportFormat): Promise<ProfileExportArtifact> {
    const response = await fetch(`${this.base}/profiles/export?format=${format}`);
    if (!response.ok) {
      throw new Error(`GET /profiles/export failed (${response.status})`);
    }
    const bytes = new Uint8Array(await response.arrayBuffer());
    return {
      filename: filenameFromDisposition(response.headers.get('Content-Disposition'), format),
      mime: response.headers.get('Content-Type') ?? mimeFor(format),
      bytes,
    };
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

  async exportLibrary(format: ProfileExportFormat): Promise<ProfileExportArtifact> {
    const { invoke } = await import('@tauri-apps/api/core');
    const artifact = await invoke<{ filename: string; mime: string; bytes: number[] }>(
      'profiles_export',
      { format },
    );
    return { ...artifact, bytes: Uint8Array.from(artifact.bytes) };
  }
}

/** Fallback MIME type when a response does not state one. */
function mimeFor(format: ProfileExportFormat): string {
  return format === 'bundle' ? 'application/zip' : 'application/toml';
}

/** Read the server's suggested filename, falling back to the engine's naming. */
function filenameFromDisposition(header: string | null, format: ProfileExportFormat): string {
  const match = header?.match(/filename="?([^"]+)"?/i);
  return match?.[1] ?? (format === 'bundle' ? 'slicer-profiles.zip' : 'profiles.toml');
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
