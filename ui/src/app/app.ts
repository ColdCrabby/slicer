import { afterNextRender, Component, Injector, inject } from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { CelebrationOverlay } from './components/celebration-overlay/celebration-overlay';
import { NotificationCenter } from './components/notification-center/notification-center';
import { UpdateBanner } from './components/update-banner/update-banner';
import { isTauriDesktop, isTauriHost } from './runtime/domain/runtime-mode.util';
import { AppVersion } from './services/app-version';
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
  private readonly injector = inject(Injector);

  constructor() {
    // Fire-and-forget: detect upgrades and surface "What's New" without
    // blocking startup. Failures are handled inside the service.
    void this.appVersion.checkForNewVersion();

    // Watch for a newer static deployment (Pages/web runtime) so a stale tab
    // gets a reload prompt even though there's no server to announce a version.
    this.appVersion.startUpdateWatch();

    // Accept models the OS hands us — "Open with Cold Crabby", a double-clicked
    // .3mf, a share sheet from Shapr3D. Started from the root component because
    // a cold launch's file is already waiting in the shell: the sooner
    // something is listening, the shorter the gap between the tap in the other
    // app and the model landing on the plate.
    //
    // Imported dynamically and only where the feature exists, so the web build
    // never downloads it — and so neither the Tauri APIs nor the slicing
    // runtime it reaches for ride into the initial bundle.
    if (isTauriHost()) {
      void this.startOpenWith();
    }

    // The Windows/Linux desktop window is created hidden so the user never sees
    // WebView2's blank, unresponsive cold-start frame (the "app hangs before it
    // works" symptom). Reveal it now that the shell has painted its first frame.
    // No-op on the web (no window) and on macOS/mobile, which stay visible from
    // the start; the desktop shell also arms a Rust-side fallback in case this
    // never runs.
    afterNextRender(() => void this.revealDesktopWindow());
  }

  private async startOpenWith(): Promise<void> {
    try {
      const { OpenWith } = await import('./services/open-with');
      this.injector.get(OpenWith).start();
    } catch {
      // Nothing to fall back to: without the shell there is no OS handing us
      // files. The app's own open/drop paths are unaffected.
    }
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
