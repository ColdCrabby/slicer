import { type Signal, computed, effect, signal } from '@angular/core';
import type { ProfileMeta } from '../../models/profile-source';
import type { LocalCollectionStore } from '../../services/profiles/local-collection-store';
import type { ProfileSaveState } from './profile-head';

/** How long "Saved" stays up once the edit has landed. */
const SAVED_MS = 2000;

/**
 * What a profile editor's header should say about the profile on screen.
 *
 * "Saving…" while the store is writing, then "Saved" for a moment once it has,
 * and nothing otherwise — including for an edit to some other profile, and for
 * the store merely reloading. The quiet confirmation is what tells someone
 * editing a built-in that the change stuck, which is the thing the old header
 * left them to doubt.
 *
 * In the browser-only runtime the store never reports a save in flight — the
 * write to this browser is immediate — so an edit goes straight to "Saved".
 *
 * Call from an injection context; it owns an effect.
 */
export function profileSaveState(
  store: LocalCollectionStore<ProfileMeta>,
  selectedId: Signal<string | null>,
): Signal<ProfileSaveState> {
  const flashing = signal<string | null>(null);
  let acknowledged = 0;

  effect((onCleanup) => {
    const edit = store.lastEdit();
    const status = store.saveStatus();
    if (!edit || status !== 'idle' || edit.at <= acknowledged) {
      return;
    }
    acknowledged = edit.at;
    flashing.set(edit.id);
    const timer = setTimeout(() => flashing.set(null), SAVED_MS);
    onCleanup(() => clearTimeout(timer));
  });

  return computed<ProfileSaveState>(() => {
    const id = selectedId();
    if (!id || store.lastEdit()?.id !== id) {
      return null;
    }
    switch (store.saveStatus()) {
      case 'error':
        return 'error';
      case 'pending':
      case 'saving':
        return 'saving';
      default:
        return flashing() === id ? 'saved' : null;
    }
  });
}
