import { Observable } from 'rxjs';

/**
 * Give up waiting for a genuinely idle moment after this long and run anyway.
 * A busy app (a slice running, a large model rendering) may never report idle,
 * and the work queued behind this is wanted before the user notices.
 */
const IDLE_TIMEOUT_MS = 3_000;

/** Fallback delay where `requestIdleCallback` is unavailable (Safari < 17). */
const FALLBACK_DELAY_MS = 2_000;

/**
 * Run `task` once the browser reports it has nothing better to do.
 *
 * This is the one place that knows how to ask for an idle moment, because the
 * ask has a sharp edge: `requestIdleCallback` is a Web IDL operation and throws
 * "Illegal invocation" if it is called through a detached reference, so it must
 * go through `globalThis`. It also does not exist in Safari before 17, which is
 * what the timer fallback covers.
 *
 * @returns a function that cancels the pending task.
 */
export function onIdle(task: () => void): () => void {
  if (typeof globalThis.requestIdleCallback !== 'function') {
    const timer = setTimeout(task, FALLBACK_DELAY_MS);
    return () => clearTimeout(timer);
  }

  const handle = globalThis.requestIdleCallback(task, { timeout: IDLE_TIMEOUT_MS });
  return () => globalThis.cancelIdleCallback?.(handle);
}

/** {@link onIdle} as a one-shot observable, for composing into a stream. */
export function whenIdle(): Observable<void> {
  return new Observable<void>((subscriber) =>
    onIdle(() => {
      subscriber.next();
      subscriber.complete();
    }),
  );
}

/**
 * Whether the user has asked us not to spend bandwidth speculatively — either
 * explicitly (Data Saver) or implicitly by being on a slow connection.
 *
 * `navigator.connection` is Chromium-only, so this is a best-effort check: when
 * nothing is reported we go ahead, which is the right default for the desktop
 * and native builds where the assets are local anyway.
 */
export function shouldConserveData(): boolean {
  const connection = (
    navigator as Navigator & {
      connection?: { saveData?: boolean; effectiveType?: string };
    }
  ).connection;

  if (!connection) {
    return false;
  }
  return (
    connection.saveData === true ||
    connection.effectiveType === '2g' ||
    connection.effectiveType === 'slow-2g'
  );
}
