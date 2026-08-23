import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MarkdownComponent } from 'ngx-markdown';
import { AppVersion } from '../../services/app-version';

/**
 * Dialog body shown after an upgrade. Renders the changelog sections gathered
 * by {@link AppVersion.whatsNew} — one heading + markdown block per release the
 * user skipped since they last ran the app.
 */
@Component({
  selector: 'nexus-whats-new',
  standalone: true,
  imports: [MarkdownComponent],
  templateUrl: './whats-new-panel.html',
  styleUrl: './whats-new-panel.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WhatsNewPanel {
  private readonly appVersion = inject(AppVersion);

  readonly entries = this.appVersion.whatsNew;
}
