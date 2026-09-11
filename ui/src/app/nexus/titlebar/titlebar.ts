import { ChangeDetectionStrategy, Component, computed, signal } from '@angular/core';
import { ConnectionState } from '../../components/connection-state/connection-state';
import { Logo } from '../../components/logo/logo';
import { WorkplateTabs } from '../../components/workplate-tabs/workplate-tabs';
import { environment } from '../../../environments/environment';
import type { RuntimeMode } from '../../runtime/domain/runtime-mode';
import {
  isTauriDesktop,
  isTauriMobile,
  resolveRuntimeMode,
} from '../../runtime/domain/runtime-mode.util';
import { Icon, IconButton, TooltipDirective } from '@coldcrabby/ui';

/**
 * Where a runtime's API reference lives, or `null` when it has no server.
 *
 * Split out so the rule is testable without standing up the title bar: only
 * the cloud runtime talks to an HTTP server, and a link that leads nowhere is
 * worse than a missing one.
 */
export function apiDocsUrlFor(mode: RuntimeMode, apiUrl: string): string | null {
  return mode === 'cloud' ? `${apiUrl}/docs` : null;
}

/**
 * Custom window title bar for the desktop shell.
 *
 * - macOS: the window uses the native "Overlay" title-bar style, so the native
 *   traffic lights remain and this bar only reserves space for them + brand.
 * - Windows/Linux: the window is frameless, so this bar draws the
 *   minimize / maximize / close controls.
 * - iOS/iPadOS: there is no window at all — no traffic lights to clear and
 *   nothing to minimize — so the bar renders as a brand strip, offset below the
 *   status bar via the safe-area inset.
 * - Web: there is no window frame; the bar renders as a slim brand strip.
 *
 * The whole bar (minus the controls) is a Tauri drag region.
 */
@Component({
  selector: 'nexus-titlebar',
  imports: [ConnectionState, Logo, WorkplateTabs, Icon, IconButton, TooltipDirective],
  templateUrl: './titlebar.html',
  styleUrl: './titlebar.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    class: 'nexus-titlebar',
    'data-tauri-drag-region': '',
    '[class.is-desktop]': 'isDesktop()',
    '[class.is-mac]': 'isMac()',
    '[class.is-mobile]': 'isMobile()',
  },
})
export class NexusTitlebar {
  readonly isMobile = signal(isTauriMobile());

  /**
   * Where this slicer's API reference lives, or `null` when there is no server
   * to have one.
   *
   * Only the cloud runtime talks to an HTTP server; the desktop app drives the
   * engine over Tauri commands and the web build runs it in a worker, so in
   * both of those the link would point at nothing. It resolves off
   * `environment.apiUrl`, which is same-origin by design — this is *this*
   * slicer's API, not a hosted copy of the docs, so a self-hosted instance
   * links to its own.
   */
  readonly apiDocsUrl = signal(apiDocsUrlFor(resolveRuntimeMode(), environment.apiUrl));
  /** A Tauri host *with* a window frame. iPad is a Tauri host but has none. */
  readonly isDesktop = signal(isTauriDesktop());
  readonly isMac = signal(this.detectMac());

  /** Custom controls only on non-mac desktop; mac keeps native traffic lights. */
  readonly showWindowControls = computed(() => this.isDesktop() && !this.isMac());
  readonly maximized = signal(false);

  async minimize(): Promise<void> {
    (await this.win())?.minimize();
  }

  async toggleMaximize(): Promise<void> {
    const w = await this.win();
    if (!w) {
      return;
    }
    await w.toggleMaximize();
    this.maximized.set(await w.isMaximized());
  }

  async close(): Promise<void> {
    (await this.win())?.close();
  }

  private async win() {
    if (!this.isDesktop()) {
      return null;
    }
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      return getCurrentWindow();
    } catch {
      return null;
    }
  }

  private detectMac(): boolean {
    if (typeof navigator === 'undefined') {
      return false;
    }
    const platform = navigator.platform ?? '';
    return /Mac/i.test(platform) || /Mac OS X/i.test(navigator.userAgent);
  }
}
