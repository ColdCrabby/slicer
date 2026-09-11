import { Provider } from '@angular/core';
import { environment } from '../../environments/environment';
import type { WorkplateSetup } from '../../generated/slicer-engine-workplate-setup-v1';
import { resolveRuntimeMode } from '../runtime/domain/runtime-mode.util';

export type { WorkplateSetup };

/**
 * Where a workplate's saved setup lives, per runtime.
 *
 * A plate records which printer, filament and process it was set up with, the
 * user's sparse override diff, and where each object sits. That belongs next to
 * the engine for the same reason the profile library does — a cloud user who
 * clears their browser should not lose their plates — so this mirrors
 * {@link ProfilePersistence} exactly: three adapters, picked from
 * {@link resolveRuntimeMode} rather than the build-time environment, because
 * the desktop app ships the `cloud` environment and only becomes `native` by
 * detecting Tauri at runtime.
 *
 * The document is deliberately thin: **references, never copies.** Three
 * profile ids, not three profiles; file ids and placements, not mesh bytes.
 * That is what lets editing a print profile reach every plate that uses it.
 */
export abstract class WorkplatePersistence {
  /**
   * Whether there is an engine behind this runtime to persist to. `false` in
   * the web build, where the browser *is* the engine and `localStorage` — held
   * by {@link WorkplateSettingsStore} — is the only copy there can be.
   */
  abstract readonly isEngineBacked: boolean;

  /** One plate's saved setup, or `null` when it was never configured. */
  abstract load(requestUuid: string): Promise<WorkplateSetup | null>;

  /** Replace one plate's saved setup. Whole-document, last writer wins. */
  abstract save(requestUuid: string, setup: WorkplateSetup): Promise<void>;
}

/** Browser-local backend (wasm): the store's own `localStorage` is the truth. */
export class BrowserWorkplatePersistence extends WorkplatePersistence {
  readonly isEngineBacked = false;
  async load(): Promise<WorkplateSetup | null> {
    return null;
  }
  async save(): Promise<void> {
    // No engine store — the workplate store already wrote localStorage.
  }
}

/** Cloud backend: REST to the slicer server's `/api/workplates/:uuid`. */
export class RemoteWorkplatePersistence extends WorkplatePersistence {
  readonly isEngineBacked = true;
  private readonly base = environment.apiUrl;

  async load(requestUuid: string): Promise<WorkplateSetup | null> {
    const response = await fetch(`${this.base}/workplates/${encodeURIComponent(requestUuid)}`, {
      headers: { Accept: 'application/json' },
    });
    if (!response.ok) {
      throw new Error(`GET /workplates failed (${response.status})`);
    }
    return (await response.json()) as WorkplateSetup;
  }

  async save(requestUuid: string, setup: WorkplateSetup): Promise<void> {
    const response = await fetch(`${this.base}/workplates/${encodeURIComponent(requestUuid)}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(setup),
      // A plate configured moments before the tab closes still reaches the
      // server: the request outlives the page that started it.
      keepalive: true,
    });
    if (!response.ok) {
      throw new Error(`PUT /workplates failed (${response.status})`);
    }
  }
}

/** Native backend: Tauri commands over the engine's own config directory. */
export class NativeWorkplatePersistence extends WorkplatePersistence {
  readonly isEngineBacked = true;

  async load(requestUuid: string): Promise<WorkplateSetup | null> {
    const { invoke } = await import('@tauri-apps/api/core');
    return await invoke<WorkplateSetup | null>('workplate_load', { requestUuid });
  }

  async save(requestUuid: string, setup: WorkplateSetup): Promise<void> {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('workplate_save', { requestUuid, setup });
  }
}

/**
 * Bind {@link WorkplatePersistence} to the backend for the active runtime mode.
 * Register once in the app providers.
 */
export function provideWorkplatePersistence(): Provider {
  return {
    provide: WorkplatePersistence,
    useFactory: (): WorkplatePersistence => {
      switch (resolveRuntimeMode()) {
        case 'native':
          return new NativeWorkplatePersistence();
        case 'cloud':
          return new RemoteWorkplatePersistence();
        default:
          return new BrowserWorkplatePersistence();
      }
    },
  };
}
