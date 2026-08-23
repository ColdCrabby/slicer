import { Injectable, signal } from '@angular/core';

export type NotificationSeverity = 'info' | 'success' | 'warning' | 'error';

/**
 * A transient status toast — fire-and-forget feedback about an action's
 * result. Rendered by the bottom-left `NotificationCenter`.
 */
export interface Notification {
  id: string;
  severity: NotificationSeverity;
  title: string;
  message?: string;
  /** When false the user can dismiss; when true the close button is hidden. */
  dismissible: boolean;
  /** Auto-dismiss after this many ms. Omit to keep until dismissed or updated. */
  autoDismissMs?: number;
}

/**
 * A determinate, long-running background task with a progress bar. Rendered by
 * the docked `TaskProgressBar` strip at the top of the scene, not as a floating
 * toast. On completion/failure the task is removed and handed off to a toast.
 */
export interface ProgressTask {
  id: string;
  title: string;
  message?: string;
  /** 0–100. */
  progress: number;
}

let _nextId = 1;

@Injectable({ providedIn: 'root' })
export class NotificationService {
  /** Transient status toasts (bottom-left center). */
  readonly notifications = signal<Notification[]>([]);
  /** Active determinate progress tasks (docked strip). */
  readonly tasks = signal<ProgressTask[]>([]);

  /** Push a simple informational toast and return its id. */
  info(title: string, message?: string, autoDismissMs = 4000): string {
    return this.push({ severity: 'info', title, message, autoDismissMs, dismissible: true });
  }

  success(title: string, message?: string, autoDismissMs = 4000): string {
    return this.push({ severity: 'success', title, message, autoDismissMs, dismissible: true });
  }

  warning(title: string, message?: string, autoDismissMs = 6000): string {
    return this.push({
      severity: 'warning',
      title,
      message,
      autoDismissMs,
      dismissible: true,
    });
  }

  error(title: string, message?: string): string {
    return this.push({ severity: 'error', title, message, dismissible: true });
  }

  /**
   * Start a determinate progress task shown in the docked strip.
   * Returns the id — call `updateProgress`, then `completeProgress` /
   * `failProgress` to finish it.
   */
  progress(title: string, message?: string): string {
    const id = String(_nextId++);
    this.tasks.update((list) => [...list, { id, title, message, progress: 0 }]);
    return id;
  }

  /** Update the progress (0–100) and optionally change the message. */
  updateProgress(id: string, progress: number, message?: string): void {
    this.tasks.update((list) =>
      list.map((t) =>
        t.id === id
          ? {
              ...t,
              progress,
              ...(message !== undefined ? { message } : {}),
            }
          : t,
      ),
    );
  }

  /** Complete a task — removes the strip entry and shows a success toast. */
  completeProgress(id: string, title: string, message?: string): void {
    this.dismissTask(id);
    this.success(title, message);
  }

  /** Fail a task — removes the strip entry and shows an error toast. */
  failProgress(id: string, title: string, message?: string): void {
    this.dismissTask(id);
    this.error(title, message);
  }

  /** Remove a progress task from the docked strip. */
  dismissTask(id: string): void {
    this.tasks.update((list) => list.filter((t) => t.id !== id));
  }

  dismiss(id: string): void {
    this.notifications.update((list) => list.filter((n) => n.id !== id));
  }

  private push(partial: Omit<Notification, 'id'>): string {
    const id = String(_nextId++);
    const notification: Notification = { id, ...partial };
    this.notifications.update((list) => [...list, notification]);

    if (notification.autoDismissMs) {
      this.scheduleAutoDismiss(id, notification.autoDismissMs);
    }

    return id;
  }

  private scheduleAutoDismiss(id: string, delayMs: number): void {
    setTimeout(() => this.dismiss(id), delayMs);
  }
}
