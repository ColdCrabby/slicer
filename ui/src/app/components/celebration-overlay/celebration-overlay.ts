import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { NotificationService } from '../../services/notifications';
import { Icon } from '../../shared/icon/icon';

/**
 * Root-mounted, non-interactive full-page flourish that plays once when a
 * high-value action succeeds (e.g. an upload lands). Driven entirely by
 * {@link NotificationService.celebration}; the service clears itself after the
 * fade-out, so this component only renders and animates.
 */
@Component({
  selector: 'nexus-celebration-overlay',
  standalone: true,
  imports: [Icon],
  templateUrl: './celebration-overlay.component.html',
  styleUrl: './celebration-overlay.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class CelebrationOverlay {
  private readonly service = inject(NotificationService);

  readonly celebration = this.service.celebration;

  /** Evenly-spaced spark angles (degrees) radiating from the badge. */
  protected readonly sparks = Array.from({ length: 12 }, (_, i) => i * 30);
}
