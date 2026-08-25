import { inject, Injectable } from '@angular/core';
import { DEFAULT_FILAMENTS } from '../models/filament.model';
import { DEFAULT_LABELS } from '../models/label.model';
import { DEFAULT_PRINT_PROFILES } from '../models/print-profile.model';
import { DEFAULT_PRINTERS } from '../models/printer.model';
import { resolveRuntimeMode } from '../runtime/domain/runtime-mode.util';
import { ProfilePersistence } from './profiles/profile-persistence';
import { Slicer } from './slicer';

/**
 * localStorage keys owned by the profile library. Cleared when the user resets
 * their profiles so a reload re-seeds the built-in defaults instead of rehydrating
 * the wiped entries from cache.
 */
const PROFILE_STORAGE_KEYS = [
  'profiles.printers',
  'profiles.filaments',
  'profiles.printProfiles',
  'profiles.labels',
  'profiles.active',
  'profiles.labelFilter',
] as const;

/**
 * Executes the irreversible "Danger Zone" actions from settings, routed to the
 * right place for the active runtime:
 *
 * - **Clear slice history** drops the engine's sessions + G-code cache (cloud
 *   REST / native command). The web/wasm runtime keeps no history, so it is a
 *   no-op there and {@link canClearHistory} is `false`.
 * - **Reset profiles** rewrites every profile category to its built-in default
 *   — both in the engine store (native/cloud) and in this browser's cache — so
 *   the user gets a clean library back.
 * - **Factory reset** does both of the above and additionally wipes *all* of
 *   this browser's app state (appearance, view, layout preferences), returning
 *   the app to a first-launch state.
 *
 * Profile resets and the factory reset reload the page so every store rehydrates
 * from the freshly-cleared state in one pass.
 */
@Injectable({ providedIn: 'root' })
export class DangerZone {
  private readonly slicer = inject(Slicer);
  private readonly persistence = inject(ProfilePersistence);

  /**
   * Whether the active runtime keeps slice history worth clearing. False on the
   * web/wasm runtime, which holds nothing on a server.
   */
  readonly canClearHistory = resolveRuntimeMode() !== 'web';

  /** Where destructive data lives, for the confirmation copy. */
  readonly storageScope: 'device' | 'server' | 'browser' = ((): 'device' | 'server' | 'browser' => {
    switch (resolveRuntimeMode()) {
      case 'native':
        return 'device';
      case 'cloud':
        return 'server';
      default:
        return 'browser';
    }
  })();

  /** Drop every slicing session and the engine's G-code cache. */
  async clearHistory(): Promise<void> {
    await this.slicer.clearHistory();
  }

  /**
   * Reset all four profile categories to their built-in defaults, then reload so
   * the stores rehydrate cleanly.
   */
  async resetProfiles(): Promise<void> {
    await this.writeDefaultProfiles();
    for (const key of PROFILE_STORAGE_KEYS) {
      localStorage.removeItem(key);
    }
    this.reload();
  }

  /**
   * Return the app to a first-launch state: clear slice history, reset profiles
   * to defaults in the engine store, wipe this browser's entire app state, then
   * reload.
   */
  async factoryReset(): Promise<void> {
    if (this.canClearHistory) {
      // Best-effort: a history failure must not block the local wipe.
      await this.slicer.clearHistory().catch(() => undefined);
    }
    await this.writeDefaultProfiles().catch(() => undefined);
    localStorage.clear();
    this.reload();
  }

  /**
   * Push the built-in defaults to the engine store for every category. A no-op
   * on the web/wasm runtime, where the store is not engine-backed.
   */
  private async writeDefaultProfiles(): Promise<void> {
    if (!this.persistence.isEngineBacked) {
      return;
    }
    await Promise.all([
      this.persistence.saveCategory('printers', DEFAULT_PRINTERS),
      this.persistence.saveCategory('filaments', DEFAULT_FILAMENTS),
      this.persistence.saveCategory('processes', DEFAULT_PRINT_PROFILES),
      this.persistence.saveCategory('labels', DEFAULT_LABELS),
    ]);
  }

  private reload(): void {
    window.location.reload();
  }
}
