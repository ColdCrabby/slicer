import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { Icon } from '../../shared/icon/icon';
import { AppVersion } from '../../services/app-version';

/**
 * A docked prompt that appears when the server announces a newer release than
 * the bundle this browser tab is running (see {@link AppVersion.updateAvailable}).
 *
 * The common cause is a redeploy while a long-lived tab stayed open: the running
 * JS/WASM is stale but the user has no reason to know they should hard-refresh.
 * This offers a single, obvious "Reload" action that force-fetches the new
 * build. It intentionally has no dismiss button — a stale UI can misbehave, so
 * reloading is the only safe resolution.
 */
@Component({
  selector: 'nexus-update-banner',
  standalone: true,
  imports: [Icon],
  templateUrl: './update-banner.html',
  styleUrl: './update-banner.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class UpdateBanner {
  private readonly appVersion = inject(AppVersion);

  readonly visible = this.appVersion.updateAvailable;
  readonly serverVersion = this.appVersion.serverVersion;

  reload(): void {
    this.appVersion.reloadForUpdate();
  }
}
