import { resolveRuntimeMode } from '../../../runtime/domain/runtime-mode.util';

/**
 * Where the profile library is persisted for the active runtime.
 *
 * - `device` (native) — saved locally, next to the engine.
 * - `server` (cloud) — saved on the slicer server; safe if this browser is
 *   wiped.
 * - `browser` (web/wasm) — kept only in this browser; losable.
 */
export type StorageMode = 'device' | 'server' | 'browser';

export interface StorageNote {
  mode: StorageMode;
  icon: string;
  /** A few words — what the Settings sidebar shows. */
  title: string;
  /** The sentence behind it — what General → Your library shows. */
  text: string;
}

/**
 * What to tell the user about where their printers, filaments and profiles
 * live, so they are reassured (or warned) about what survives clearing this
 * browser. Mirrors the transport table in `src/profiles/README.md`.
 */
export function storageNote(): StorageNote {
  switch (resolveRuntimeMode()) {
    case 'native':
      return {
        mode: 'device',
        icon: 'hard-drive',
        title: 'Saved on this device',
        text: 'Saved on this computer, next to the slicer. Appearance and view preferences stay in this window.',
      };
    case 'cloud':
      return {
        mode: 'server',
        icon: 'cloud-check',
        title: 'Saved on the slicer',
        text: "Saved on the slicer server, so it's safe if you clear this browser. Only appearance and view preferences live here.",
      };
    default:
      return {
        mode: 'browser',
        icon: 'database',
        title: 'Stored in this browser',
        text: 'Kept in this browser only. Clearing site data, resetting the browser or reinstalling it will erase it — export a copy to keep one.',
      };
  }
}
