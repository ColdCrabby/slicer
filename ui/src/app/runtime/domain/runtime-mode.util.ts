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
