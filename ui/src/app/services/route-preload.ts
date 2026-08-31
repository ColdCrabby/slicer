import { Injectable } from '@angular/core';
import type { PreloadingStrategy, Route } from '@angular/router';
import { Observable, of, switchMap } from 'rxjs';
import { shouldConserveData, whenIdle } from './idle';

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
