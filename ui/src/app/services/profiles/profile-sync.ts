import { Injectable, computed, inject } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { SlicerConnection } from '../slicer-connection';
import { FilamentsStore } from './filaments-store';
import { LabelsStore } from './labels-store';
import { PrintProfilesStore } from './print-profiles-store';
import { PrintersStore } from './printers-store';

/** Aggregated engine-sync state across every profile category. */
export type ProfileSyncStatus = 'idle' | 'loading' | 'saving' | 'error';

/**
 * Keeps the profile stores fresh when the engine's library changes from
 * another client or browser tab, and exposes an aggregated sync status for a
 * subtle "loading / saving" indicator.
 *
 * The engine broadcasts a `ProfilesChanged` message over the WebSocket after
 * any `PUT /api/profiles/:kind`; this service maps the changed category token
 * to its store and triggers a cache-bypassing reload. In web/native mode the
 * connection's `messages$` is `EMPTY`, so this is inert — those runtimes have
 * no second client to diverge from.
 */
@Injectable({ providedIn: 'root' })
export class ProfileSync {
  private readonly connection = inject(SlicerConnection);
  private readonly printers = inject(PrintersStore);
  private readonly filaments = inject(FilamentsStore);
  private readonly processes = inject(PrintProfilesStore);
  private readonly labels = inject(LabelsStore);

  private readonly stores = [this.printers, this.filaments, this.processes, this.labels];

  /**
   * Aggregated sync state, worst-first: a failed save needs attention over an
   * in-flight save, which is more relevant than a background load. `idle`
   * renders nothing.
   */
  readonly status = computed<ProfileSyncStatus>(() => {
    if (this.stores.some((s) => s.saveStatus() === 'error' || s.loadStatus() === 'error')) {
      return 'error';
    }
    if (this.stores.some((s) => s.saveStatus() === 'pending' || s.saveStatus() === 'saving')) {
      return 'saving';
    }
    if (this.stores.some((s) => s.loading())) {
      return 'loading';
    }
    return 'idle';
  });

  constructor() {
    this.connection.messages$.pipe(takeUntilDestroyed()).subscribe((msg) => {
      if (msg.type === 'ProfilesChanged') {
        void this.reload(msg.kind);
      }
    });
  }

  private reload(kind: string): Promise<void> {
    switch (kind) {
      case 'printers':
        return this.printers.reload();
      case 'filaments':
        return this.filaments.reload();
      case 'processes':
        return this.processes.reload();
      case 'labels':
        return this.labels.reload();
      default:
        return Promise.resolve();
    }
  }
}
