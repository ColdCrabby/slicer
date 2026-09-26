import { DOCUMENT, Injectable, inject } from '@angular/core';
import { isTauriHost } from '../runtime/domain/runtime-mode.util';

/**
 * Open a link outside the app: the system browser from a native shell, a new
 * tab otherwise.
 *
 * Inside Tauri the webview cannot open a new window itself (WKWebView on macOS
 * and iPadOS simply drops `window.open` and `target="_blank"`), so the URL is
 * handed to the opener plugin, which asks the OS to open it.
 */
export function openExternal(url: string): void {
  if (isTauriHost()) {
    void import('@tauri-apps/plugin-opener').then(({ openUrl }) => openUrl(url));
    return;
  }
  window.open(url, '_blank', 'noopener,noreferrer');
}

/**
 * Sends every http(s) link click in a native shell to the system browser.
 *
 * **Why a document-level listener rather than a directive.** Links come from
 * templates across the app and from rendered markdown, which no directive can
 * reach; a directive would also have to be imported by every component that
 * renders a link, where the next one added would silently miss out (the old
 * `ExternalLinkDirective` was imported by none). One listener covers them all.
 * In a browser it does nothing: the anchor's own `target` already behaves.
 */
@Injectable({ providedIn: 'root' })
export class ExternalLinks {
  private readonly document = inject(DOCUMENT);

  constructor() {
    if (!isTauriHost()) {
      return;
    }
    this.document.addEventListener('click', this.onClick);
  }

  private readonly onClick = (event: MouseEvent): void => {
    if (event.defaultPrevented || event.button !== 0) {
      return;
    }
    const anchor = (event.target as Element | null)?.closest?.<HTMLAnchorElement>('a[href]');
    // Same-origin links are the app's own routes: under `tauri dev` they are
    // served over http://localhost too, and must stay in the webview.
    if (
      !anchor ||
      !/^https?:\/\//i.test(anchor.href) ||
      anchor.origin === this.document.location.origin
    ) {
      return;
    }
    event.preventDefault();
    openExternal(anchor.href);
  };
}
