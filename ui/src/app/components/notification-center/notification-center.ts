import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { Icon } from '@coldcrabby/ui';
import { NotificationService, type Notification } from '../../services/notifications';

@Component({
  selector: 'nexus-notification-center',
  standalone: true,
  imports: [Icon],
  templateUrl: './notification-center.component.html',
  styleUrl: './notification-center.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class NotificationCenter {
  readonly #service = inject(NotificationService);

  readonly notifications = this.#service.notifications;

  /** Hold a toast's countdown while the pointer or focus is on it. */
  protected pause(notification: Notification): void {
    this.#service.pauseAutoDismiss(notification.id);
  }

  /** Resume it once they move away. */
  protected resume(notification: Notification): void {
    this.#service.resumeAutoDismiss(notification.id);
  }

  dismiss(notification: Notification): void {
    if (!notification.dismissible) {
      return;
    }
    this.#service.dismiss(notification.id);
  }

  iconFor(severity: Notification['severity']): string {
    switch (severity) {
      case 'success':
        return 'check-circle';
      case 'warning':
        return 'warning-triangle';
      case 'error':
        return 'xmark-circle';
      default:
        return 'info-circle';
    }
  }

  trackById(_index: number, item: Notification): string {
    return item.id;
  }
}
