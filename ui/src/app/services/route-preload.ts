import { Injectable } from '@angular/core';
import type { PreloadingStrategy, Route } from '@angular/router';
import { Observable, of, switchMap } from 'rxjs';

/**
 * Give up waiting for a genuinely idle moment after this long and preload
 * anyway. A busy app (a slice running, a large model rendering) may never
 * report idle, and a user on that app is exactly the one about to navigate.
 */
const IDLE_TIMEOUT_MS = 3_000;

/** Fallback delay where `requestIdleCallback` is unavailable (Safari < 17). */
const FALLBACK_DELAY_MS = 2_000;

/**
 * Fetch lazy route chunks in the background, once the app has nothing better
 * to do.
 *
 * Splitting every screen into its own chunk is what keeps the initial download
 * small, but on its own it just moves the wait: the first click on "Slice" pays
 * for the 3D viewer. Preloading closes that gap — by the time the user reaches
 * for a destination, its code is usually already in the browser's cache and the
 * navigation is instant.
 *
 * Two properties make this a win rather than a wash:
 *
 * - **It waits for idle**, so the fetches never compete with the first paint or
 *   with an in-flight slice. Angular's built-in `PreloadAllModules` starts the
 *   moment the first navigation ends, which is precisely when the app is
 *   busiest.
 * - **It respects the connection.** Preloading spends bandwidth on a guess. On
 *   a metered or slow link that guess is rude, so it is skipped and those users
 *   simply load on demand — where {@link NavigationProgress} tells them what is
 *   happening.
 *
 * Opt a route out with `data: { preload: false }` when its chunk is large and
 * rarely wanted.
 */
@Injectable({ providedIn: 'root' })
export class IdleRoutePreload implements PreloadingStrategy {
  preload(route: Route, load: () => Observable<unknown>): Observable<unknown> {
    if (route.data?.['preload'] === false || shouldConserveData()) {
      return of(null);
    }
    return whenIdle().pipe(switchMap(() => load()));
  }
}

/** Emits once the browser reports an idle moment (or the timeout expires). */
function whenIdle(): Observable<void> {
  return new Observable<void>((subscriber) => {
    const done = () => {
      subscriber.next();
      subscriber.complete();
    };

    // Called as a member of `globalThis` rather than through a detached
    // reference: `requestIdleCallback` is a Web IDL operation and throws
    // "Illegal invocation" when its `this` is not the window.
    if (typeof globalThis.requestIdleCallback !== 'function') {
      const timer = setTimeout(done, FALLBACK_DELAY_MS);
      return () => clearTimeout(timer);
    }

    const handle = globalThis.requestIdleCallback(done, { timeout: IDLE_TIMEOUT_MS });
    return () => globalThis.cancelIdleCallback?.(handle);
  });
}

/**
 * Whether the user has asked us not to spend bandwidth speculatively — either
 * explicitly (Data Saver) or implicitly by being on a slow connection.
 *
 * `navigator.connection` is Chromium-only, so this is a best-effort check: when
 * nothing is reported we preload, which is the right default for the desktop
 * and native builds where the assets are local anyway.
 */
function shouldConserveData(): boolean {
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
