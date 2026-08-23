import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { NotificationService, type ProgressTask } from '../../services/notifications';

@Component({
  selector: 'nexus-task-progress-bar',
  standalone: true,
  imports: [],
  templateUrl: './task-progress-bar.component.html',
  styleUrl: './task-progress-bar.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class TaskProgressBar {
  readonly #service = inject(NotificationService);

  readonly tasks = this.#service.tasks;

  trackById(_index: number, item: ProgressTask): string {
    return item.id;
  }
}
