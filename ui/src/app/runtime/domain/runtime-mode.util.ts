import { environment } from '../../../environments/environment';
import type { RuntimeMode } from './runtime-mode';

/**
 * The active runtime mode, resolved the same way everywhere.
 *
 * `environment.runtimeMode` is only the *web* fallback (`cloud` vs `web`): the
 * desktop build ships the `cloud` environment and becomes `native` purely by
 * detecting the Tauri host at runtime. So a build-time constant is not enough —
 * anything that must know "am I native?" has to detect Tauri, exactly as
 * {@link SlicerService} does for slicing. Keep this the single implementation
 * so the persistence backend, the settings notice, and the slicer never
 * disagree about which runtime they are in.
 */
export function resolveRuntimeMode(): RuntimeMode {
  return isTauriHost() ? 'native' : environment.runtimeMode;
}

/** True when running inside the Tauri desktop shell. */
export function isTauriHost(): boolean {
  const globals = globalThis as unknown as {
    __TAURI__?: unknown;
    __TAURI_INTERNALS__?: unknown;
    navigator?: { userAgent?: string };
  };
  return Boolean(
    globals.__TAURI__ ||
    globals.__TAURI_INTERNALS__ ||
    globals.navigator?.userAgent?.includes('Tauri'),
  );
}

/**
 * True when the Tauri host is a mobile OS (iOS/iPadOS today).
 *
 * This is deliberately *not* a runtime mode: an iPad runs the same Rust engine
 * over the same `tauri::invoke` bridge, so {@link resolveRuntimeMode} still
 * reports `native` and every slicing/persistence decision stays identical. What
 * differs is the shell — there is no resizable, decorated window — so anything
 * that draws or drives window chrome has to ask this question separately.
 *
 * iPadOS is the awkward case: its user agent says `like Mac OS X` and
 * `navigator.platform` can report a Mac, so a naive platform sniff classifies an
 * iPad as a Mac desktop. Touch points are what actually distinguish it.
 */
export function isTauriMobile(): boolean {
  if (!isTauriHost()) {
    return false;
  }
  const nav = (globalThis as unknown as { navigator?: Navigator }).navigator;
  if (!nav) {
    return false;
  }
  if (/iPad|iPhone|iPod|Android/i.test(nav.userAgent ?? '')) {
    return true;
  }
  return /Mac/i.test(nav.platform ?? '') && (nav.maxTouchPoints ?? 0) > 1;
}
