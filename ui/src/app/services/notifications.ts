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

/**
 * A one-shot, full-page celebratory flourish (non-interactive). Used to make a
 * successful action impossible to miss — e.g. a finished upload. Rendered by
 * the root `CelebrationOverlay` and auto-cleared after {@link CELEBRATION_MS}.
 */
export interface Celebration {
  id: string;
  title: string;
  message?: string;
  /** Iconoir icon shown in the burst badge. */
  icon: string;
}

/** Lifetime (ms) of a celebration overlay — matches its fade-out keyframes. */
export const CELEBRATION_MS = 2200;

/**
 * Most toasts on screen at once.
 *
 * The stack grows upward from the bottom-left corner with nothing to stop it,
 * so a burst — a multi-model import, a run of failures — climbed over the
 * settings panel and the object list and eventually off the top of the window.
 * Oldest goes first: the newest message is the one the user is waiting on.
 */
const MAX_VISIBLE = 4;

/**
 * How long an error stays before dismissing itself.
 *
 * Errors used to persist until clicked, which reads as tidy until three of them
 * are covering the plate and the user has already moved on. Long enough to read
 * twice, short enough to clear up after itself.
 */
const ERROR_DISMISS_MS = 12000;

let _nextId = 1;

@Injectable({ providedIn: 'root' })
export class NotificationService {
  /** Transient status toasts (bottom-left center). */
  readonly notifications = signal<Notification[]>([]);
  /** Active determinate progress tasks (docked strip). */
  readonly tasks = signal<ProgressTask[]>([]);
  /** The active full-page celebration, or `null` when none is playing. */
  readonly celebration = signal<Celebration | null>(null);

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
    return this.push({
      severity: 'error',
      title,
      message,
      autoDismissMs: ERROR_DISMISS_MS,
      dismissible: true,
    });
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

  /**
   * Play a full-page celebration overlay. Supersedes any in-flight one and
   * auto-clears after {@link CELEBRATION_MS}.
   */
  celebrate(title: string, message?: string, icon = 'check-circle'): void {
    const id = String(_nextId++);
    this.celebration.set({ id, title, message, icon });
    setTimeout(() => {
      if (this.celebration()?.id === id) {
        this.celebration.set(null);
      }
    }, CELEBRATION_MS);
  }

  dismiss(id: string): void {
    this.pauseAutoDismiss(id);
    this.notifications.update((list) => list.filter((n) => n.id !== id));
  }

  /**
   * Hold the auto-dismiss countdown — while a toast is hovered or focused, so a
   * message cannot evaporate mid-sentence while it is being read.
   */
  pauseAutoDismiss(id: string): void {
    const handle = this.#timers.get(id);
    if (handle !== undefined) {
      clearTimeout(handle);
      this.#timers.delete(id);
    }
  }

  /** Restart the countdown a {@link pauseAutoDismiss} held. */
  resumeAutoDismiss(id: string): void {
    if (this.#timers.has(id)) {
      return;
    }
    const remaining = this.notifications().find((n) => n.id === id)?.autoDismissMs;
    if (remaining) {
      this.scheduleAutoDismiss(id, remaining);
    }
  }

  readonly #timers = new Map<string, ReturnType<typeof setTimeout>>();

  private push(partial: Omit<Notification, 'id'>): string {
    const id = String(_nextId++);
    const notification: Notification = { id, ...partial };

    this.notifications.update((list) => {
      // An identical message repeated (the same failure retried, the same file
      // re-imported) replaces its predecessor rather than stacking a duplicate.
      const deduped = list.filter(
        (n) => !(n.title === notification.title && n.message === notification.message),
      );
      return [...deduped, notification].slice(-MAX_VISIBLE);
    });

    if (notification.autoDismissMs) {
      this.scheduleAutoDismiss(id, notification.autoDismissMs);
    }

    return id;
  }

  private scheduleAutoDismiss(id: string, delayMs: number): void {
    this.pauseAutoDismiss(id);
    this.#timers.set(
      id,
      setTimeout(() => {
        this.#timers.delete(id);
        this.dismiss(id);
      }, delayMs),
    );
  }
}
