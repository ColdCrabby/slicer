import { Component, inject } from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { NotificationCenter } from './components/notification-center/notification-center';
import { UpdateBanner } from './components/update-banner/update-banner';
import { AppVersion } from './services/app-version';
import { WasmPerformanceNotice } from './services/wasm-performance-notice';
import { DialogOutlet } from './shared/dialog/dialog-outlet';

@Component({
  selector: 'nexus-root',
  standalone: true,
  imports: [RouterOutlet, NotificationCenter, UpdateBanner, DialogOutlet],
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
  }
}
