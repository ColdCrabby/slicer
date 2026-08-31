import { DestroyRef, Injectable, computed, inject, signal } from '@angular/core';
import {
  NavigationCancel,
  NavigationEnd,
  NavigationError,
  NavigationSkipped,
  NavigationStart,
  Router,
} from '@angular/router';
import { AppVersion } from './app-version';
import { Logger } from './logger';

/**
 * How long a navigation may run before the user is told it is happening.
 *
 * Most navigations resolve in a frame or two — their chunk is already cached,
 * or was preloaded — and flashing a progress bar for 20 ms reads as a glitch,
 * not as feedback. Anything past this threshold is long enough that silence
 * would read as a dead click instead.
 */
const SHOW_AFTER_MS = 120;

/**
 * How long the bar stays up after the navigation finishes.
 *
 * The bar animates from "in progress" to "complete"; cutting it the instant the
 * route resolves would leave a half-drawn line snapping away. This is purely
 * the exit animation's window.
 */
const SETTLE_MS = 180;

/**
 * Tracks in-flight route navigations so the UI can admit when it is waiting.
 *
 * Every screen below the app shell is a lazily-loaded chunk (see
 * {@link APP_ROUTES}), which is what keeps the initial download small — but it
 * also means a click on "Slice" can involve a real network fetch. Angular
 * renders nothing at all until that resolves, so without this the app simply
 * freezes on the old screen and the user clicks again.
 *
 * The service holds *when* to speak, not how: {@link RouteProgress} draws the
 * bar and the navigation rails mark the destination being fetched, all from
 * {@link visiblePendingUrl}. Fast navigations never set it, so an instant
 * transition stays visually silent.
 */
@Injectable({ providedIn: 'root' })
export class NavigationProgress {
  private readonly router = inject(Router);
  private readonly appVersion = inject(AppVersion);
  private readonly log = inject(Logger).scope('NavigationProgress');

  /** URL of the navigation in flight, set the moment it starts. */
  private readonly pendingUrl = signal<string | null>(null);

  /** Whether the in-flight navigation has run long enough to be worth showing. */
  private readonly slow = signal(false);

  /** Set while the completed bar plays its exit animation. */
  private readonly settling = signal(false);

  private showTimer: ReturnType<typeof setTimeout> | null = null;
  private settleTimer: ReturnType<typeof setTimeout> | null = null;

  /**
   * The URL being navigated to, but only once the wait is worth mentioning.
   * `null` for an idle app *and* for a navigation fast enough that the user
   * would never have noticed it.
   */
  readonly visiblePendingUrl = computed(() => (this.slow() ? this.pendingUrl() : null));

  /** Whether the progress bar should be on screen at all. */
  readonly active = computed(() => this.visiblePendingUrl() !== null);

  /** Whether the in-flight navigation has finished and is animating out. */
  readonly complete = computed(() => this.settling());

  constructor() {
    const subscription = this.router.events.subscribe((event) => {
      if (event instanceof NavigationStart) {
        this.begin(event.url);
        return;
      }

      if (event instanceof NavigationError) {
        this.reportFailure(event);
        this.end();
        return;
      }

      if (
        event instanceof NavigationEnd ||
        event instanceof NavigationCancel ||
        event instanceof NavigationSkipped
      ) {
        this.end();
      }
    });

    inject(DestroyRef).onDestroy(() => {
      subscription.unsubscribe();
      this.clearTimers();
    });
  }

  /**
   * Whether the navigation currently being waited on lands under `path`.
   *
   * `'/'` matches only itself — every URL is "under" the root, so a prefix test
   * would light up the Home rail item on the way to anywhere.
   */
  isPendingUnder(path: string): boolean {
    const url = this.visiblePendingUrl();
    if (url === null) {
      return false;
    }
    const target = url.split(/[?#]/, 1)[0];
    if (path === '/') {
      return target === '/' || target === '';
    }
    return target === path || target.startsWith(`${path}/`);
  }

  private begin(url: string): void {
    this.clearTimers();
    this.settling.set(false);
    this.pendingUrl.set(url);
    this.slow.set(false);
    this.showTimer = setTimeout(() => this.slow.set(true), SHOW_AFTER_MS);
  }

  private end(): void {
    this.clearTimers();

    // Nothing was ever shown, so there is nothing to animate out.
    if (!this.slow()) {
      this.pendingUrl.set(null);
      return;
    }

    this.settling.set(true);
    this.settleTimer = setTimeout(() => {
      this.settling.set(false);
      this.slow.set(false);
      this.pendingUrl.set(null);
    }, SETTLE_MS);
  }

  private clearTimers(): void {
    if (this.showTimer !== null) {
      clearTimeout(this.showTimer);
      this.showTimer = null;
    }
    if (this.settleTimer !== null) {
      clearTimeout(this.settleTimer);
      this.settleTimer = null;
    }
  }

  /**
   * A navigation that died fetching its chunk means this tab's asset manifest
   * no longer matches what the server has — the classic symptom of a redeploy
   * under a long-lived tab, where the hashed chunk filenames this bundle asks
   * for have already been swept away.
   *
   * The remedy is the same one the update banner already offers, so it is
   * raised here rather than invented again: a reload fetches a consistent
   * bundle. Any other navigation failure (a guard throwing, a resolver
   * rejecting) is logged and left alone — reloading would not help.
   */
  private reportFailure(event: NavigationError): void {
    if (!isChunkLoadError(event.error)) {
      this.log.warn(`Navigation to ${event.url} failed`, event.error);
      return;
    }

    this.log.warn(
      `Could not load the code for ${event.url} — this tab's bundle looks stale`,
      event.error,
    );
    this.appVersion.reportStaleAssets();
  }
}

/**
 * Whether `error` is a browser refusing to load a JavaScript module.
 *
 * There is no standard error type for this. Chromium throws a `TypeError`
 * ("Failed to fetch dynamically imported module"), Firefox an "error loading
 * dynamically imported module", Safari an "Importing a module script failed",
 * and bundler-generated loaders often use the name `ChunkLoadError`. Matching
 * the message text is unlovely but it is the only signal available, and getting
 * it wrong only costs an unnecessary reload prompt.
 */
function isChunkLoadError(error: unknown): boolean {
  if (typeof error !== 'object' || error === null) {
    return false;
  }
  const { name, message } = error as { name?: unknown; message?: unknown };
  if (name === 'ChunkLoadError') {
    return true;
  }
  if (typeof message !== 'string') {
    return false;
  }
  const text = message.toLowerCase();
  return (
    text.includes('dynamically imported module') ||
    text.includes('importing a module script failed') ||
    text.includes('failed to fetch dynamically')
  );
}
