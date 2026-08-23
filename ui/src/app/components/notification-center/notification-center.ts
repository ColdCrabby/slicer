import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { Icon } from '../../shared/icon/icon';
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
