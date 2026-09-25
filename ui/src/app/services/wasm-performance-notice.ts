import { inject, Injectable } from '@angular/core';
import { environment } from '../../environments/environment';
import { BrowserStorage } from './browser-storage';
import { NotificationService } from './notifications';

/**
 * localStorage key recording that the notice has been seen. Once per browser,
 * not per session: the advice does not change between visits, and a reminder
 * on every return visit reads as nagging.
 */
const NOTICE_SEEN_KEY = 'slicer:wasm-perf-notice-seen';

/** Where the desktop builds are published. */
const DESKTOP_DOWNLOAD_URL = 'https://github.com/ColdCrabby/slicer/releases/latest';

/**
 * Tells a visitor to the WebAssembly web build, once, that the whole slicer is
 * running in their tab and the desktop app is much faster.
 *
 * Raised when the first model lands on the plate, not when the app starts —
 * with something to slice, "this is slower than the desktop app" is advice
 * rather than trivia. It is a notice over the plate with a link, not a dialog:
 * the model the user just dropped is the thing they came to see, and a modal
 * over it with only "Got it" to press stood between them and it.
 *
 * Only the `web` runtime (the full WASM web bundle) is affected — the native
 * (Tauri) and cloud runtimes never see it.
 */
@Injectable({ providedIn: 'root' })
export class WasmPerformanceNotice {
  private readonly storage = inject(BrowserStorage);
  private readonly notifications = inject(NotificationService);

  /** Safe to call on every model load — a no-op off the web build or once seen. */
  maybeShow(): void {
    if (environment.runtimeMode !== 'web') {
      return;
    }
    if (this.storage.get(NOTICE_SEEN_KEY)()) {
      return;
    }
    this.storage.write(NOTICE_SEEN_KEY, '1');

    this.notifications.note('info', 'Slicing in your browser', {
      message:
        'Everything runs in this tab, so big plates are slow. The desktop app is much faster.',
      icon: 'cpu',
      autoDismissMs: null,
      action: {
        label: 'Get the desktop app',
        run: () => window.open(DESKTOP_DOWNLOAD_URL, '_blank', 'noopener,noreferrer'),
      },
    });
  }
}
