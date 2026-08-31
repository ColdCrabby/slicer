import { inject, Injectable } from '@angular/core';
import { environment } from '../../environments/environment';
import { WasmPerformancePanel } from '../components/wasm-performance-notice/wasm-performance-panel';
import { BrowserStorage } from './browser-storage';
import { Dialog } from './dialog';

/**
 * sessionStorage key recording that the WASM performance notice has been shown
 * in the current browser session. Session-scoped on purpose — the reminder
 * reappears in a fresh tab/session but never nags twice within one.
 */
const NOTICE_SEEN_KEY = 'slicer:wasm-perf-notice-seen';

/**
 * Surfaces a one-time-per-session heads-up on the WebAssembly web build that
 * running the slicer entirely in the browser costs a lot of performance, and
 * points users at the native app for full speed.
 *
 * Raised when the first model lands on the plate, not when the app starts.
 * That is where the message earns its interruption: the visitor now has
 * something to slice, so "this will be slower than the desktop app" is advice
 * rather than trivia. Shown on arrival it was the first thing a new visitor
 * met, before they had seen the app at all — and, being the largest block of
 * text on screen, it also *was* the page's Largest Contentful Paint, so the
 * whole site measured as loading however long the dialog took to appear.
 *
 * Only the `web` runtime (the full WASM web bundle) is affected — the native
 * (Tauri) and cloud runtimes never see it.
 */
@Injectable({ providedIn: 'root' })
export class WasmPerformanceNotice {
  private readonly storage = inject(BrowserStorage);
  private readonly dialog = inject(Dialog);

  /**
   * Show the WASM performance notice once per browser session. Safe to call on
   * every model load — it's a no-op off the web build or when already shown
   * this session.
   */
  maybeShow(): void {
    if (environment.runtimeMode !== 'web') {
      return;
    }

    if (this.storage.get(NOTICE_SEEN_KEY, 'session')()) {
      return;
    }

    // Record before showing so a mid-animation refresh can't reopen it.
    this.storage.write(NOTICE_SEEN_KEY, '1', 'session');

    this.dialog.alert({
      title: 'Running in your browser',
      confirmLabel: 'Got it',
      content: WasmPerformancePanel,
      preferredWidth: '560px',
    });
  }
}
