import { Component, inject } from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { NotificationCenter } from './components/notification-center/notification-center';
import { AppVersion } from './services/app-version';
import { DialogOutlet } from './shared/dialog/dialog-outlet';

@Component({
  selector: 'nexus-root',
  standalone: true,
  imports: [RouterOutlet, NotificationCenter, DialogOutlet],
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App {
  private readonly appVersion = inject(AppVersion);

  constructor() {
    // Fire-and-forget: detect upgrades and surface "What's New" without
    // blocking startup. Failures are handled inside the service.
    void this.appVersion.checkForNewVersion();
  }
}
