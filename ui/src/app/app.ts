import { afterNextRender, Component, inject } from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { CelebrationOverlay } from './components/celebration-overlay/celebration-overlay';
import { NotificationCenter } from './components/notification-center/notification-center';
import { UpdateBanner } from './components/update-banner/update-banner';
import { isTauriDesktop } from './runtime/domain/runtime-mode.util';
import { AppVersion } from './services/app-version';
import { WasmPerformanceNotice } from './services/wasm-performance-notice';
import { DialogOutlet } from './shared/dialog/dialog-outlet';

@Component({
  selector: 'nexus-root',
  standalone: true,
  imports: [RouterOutlet, NotificationCenter, CelebrationOverlay, UpdateBanner, DialogOutlet],
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App {
  private readonly appVersion = inject(AppVersion);
  private readonly wasmPerfNotice = inject(WasmPerformanceNotice);

  constructor() {
    // Fire-and-forget: detect upgrades and surface "What's New" without
    // blocking startup. Failures are handled inside the service.
    void this.appVersion.checkForNewVersion();

    // On the WASM web build only, remind the user once per session that
    // in-browser slicing trades performance for zero install.
    this.wasmPerfNotice.maybeShow();

    // Watch for a newer static deployment (Pages/web runtime) so a stale tab
    // gets a reload prompt even though there's no server to announce a version.
    this.appVersion.startUpdateWatch();

    // The Windows/Linux desktop window is created hidden so the user never sees
    // WebView2's blank, unresponsive cold-start frame (the "app hangs before it
    // works" symptom). Reveal it now that the shell has painted its first frame.
    // No-op on the web (no window) and on macOS/mobile, which stay visible from
    // the start; the desktop shell also arms a Rust-side fallback in case this
    // never runs.
    afterNextRender(() => void this.revealDesktopWindow());
  }

  private async revealDesktopWindow(): Promise<void> {
    if (!isTauriDesktop()) {
      return;
    }
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      const window = getCurrentWindow();
      await window.show();
      await window.setFocus();
    } catch {
      // Window API unavailable or the call was rejected — the Rust safety-net
      // timer will still reveal the window.
    }
  }
}
