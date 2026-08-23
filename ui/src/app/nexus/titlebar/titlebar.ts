import { ChangeDetectionStrategy, Component, computed, signal } from '@angular/core';
import { Logo } from '../../components/logo/logo';
import { Icon } from '../../shared/icon/icon';
import { IconButton } from '../../ui/icon-button/icon-button';

/**
 * Custom window title bar for the desktop shell.
 *
 * - macOS: the window uses the native "Overlay" title-bar style, so the native
 *   traffic lights remain and this bar only reserves space for them + brand.
 * - Windows/Linux: the window is frameless, so this bar draws the
 *   minimize / maximize / close controls.
 * - Web: there is no window frame; the bar renders as a slim brand strip.
 *
 * The whole bar (minus the controls) is a Tauri drag region.
 */
@Component({
  selector: 'nexus-titlebar',
  imports: [Logo, Icon, IconButton],
  templateUrl: './titlebar.html',
  styleUrl: './titlebar.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    class: 'nexus-titlebar',
    'data-tauri-drag-region': '',
    '[class.is-desktop]': 'isDesktop()',
    '[class.is-mac]': 'isMac()',
  },
})
export class NexusTitlebar {
  readonly isDesktop = signal(this.detectDesktop());
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

  private detectDesktop(): boolean {
    return (
      typeof globalThis !== 'undefined' &&
      ('__TAURI_INTERNALS__' in globalThis || '__TAURI__' in globalThis)
    );
  }

  private detectMac(): boolean {
    if (typeof navigator === 'undefined') {
      return false;
    }
    const platform = navigator.platform ?? '';
    return /Mac/i.test(platform) || /Mac OS X/i.test(navigator.userAgent);
  }
}
