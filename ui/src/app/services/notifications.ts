import { Injectable, computed, inject, signal } from '@angular/core';
import { Viewport } from './viewport';

/** Visual severity of a {@link Notice}. Mirrors the `InlineNotice` tones. */
export type NoticeTone = 'info' | 'success' | 'warning' | 'danger';

/**
 * Where a notice is drawn.
 *
 * - `scene` — the centred strip under the view toolbar, in the slice workspace.
 * - `window` — the dock at the bottom of the window, for the rare message that
 *   is not about a plate (or is raised from a route that has no scene).
 *
 * Call sites do not choose this; {@link NotificationService} does, from whether
 * a scene host is mounted and how much room the window has. On a phone the two
 * collapse into one. See {@link NotificationService.registerSceneHost}.
 */
export type NoticePlacement = 'scene' | 'window';

/** A single button carried by a notice. Its label is the verb, not "OK". */
export interface NoticeAction {
  label: string;
  run: () => void;
}

/**
 * One message, wherever it is shown.
 *
 * There is deliberately **one** shape for every announcement in the app — a
 * status note, a running job, a prompt that wants an answer — so that the same
 * event cannot be told twice in two different visual languages. What varies is
 * the tone, whether `progress` is running, and whether an `action` is offered.
 */
export interface Notice {
  id: string;
  tone: NoticeTone;
  title: string;
  message?: string;
  /** Iconoir name; defaults to the tone's icon. */
  icon?: string;
  /** 0–100 while a job runs, `null` for a plain note. */
  progress: number | null;
  /** The one thing this notice offers to do about itself. */
  action?: NoticeAction;
  /**
   * Run when the notice is put away rather than acted on. For a prompt this is
   * the *second answer* — "keep mine", "not now" — which is why a prompt needs
   * only one button.
   */
  onDismiss?: () => void;
  /** When false the close button is hidden and the notice must resolve itself. */
  dismissible: boolean;
  /** Auto-dismiss after this many ms; `null` to stay until dismissed. */
  autoDismissMs: number | null;
  placement: NoticePlacement;
}

/**
 * A one-shot, full-page celebratory flourish (non-interactive), reserved for
 * the moment a print actually starts on a machine. Rendered by the root
 * `CelebrationOverlay` and auto-cleared after {@link CELEBRATION_MS}.
 *
 * It is the only surface allowed to speak at the same time as a notice, and
 * only because it says something the notice does not: the job left the app.
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
 * Most notices on screen at once, per placement.
 *
 * The stack grows with nothing to stop it, so a burst — a multi-model import, a
 * run of failures — climbed over the chrome around it and eventually off the
 * edge of the window. Oldest goes first: the newest message is the one the user
 * is waiting on.
 */
const MAX_VISIBLE = 3;

/** Default lifetimes by tone, in ms. */
const TONE_DISMISS_MS: Record<NoticeTone, number> = {
  info: 4000,
  success: 4000,
  warning: 6000,
  /**
   * Errors used to persist until clicked, which reads as tidy until three of
   * them are covering the plate and the user has already moved on. Long enough
   * to read twice, short enough to clear up after itself.
   */
  danger: 12000,
};

/** How long a resolved job holds before leaving, by outcome. */
const RESOLVED_HOLD_MS: Record<NoticeTone, number> = {
  info: 4000,
  success: 4000,
  warning: 6000,
  danger: 8000,
};

let _nextId = 1;

/** Optional extras for {@link NotificationService.note}. */
export interface NoticeOptions {
  message?: string;
  icon?: string;
  action?: NoticeAction;
  onDismiss?: () => void;
  /**
   * Force the window dock even when the scene is mounted. For messages that
   * are not about the plate — an app update, a lost engine.
   */
  scope?: 'app';
  /** Override the tone's default lifetime; `null` keeps it until dismissed. */
  autoDismissMs?: number | null;
  /** Hide the close button. Only for a notice that resolves itself. */
  dismissible?: boolean;
}

/**
 * The app's single voice.
 *
 * Every announcement goes through here and lands in one of two docks, and the
 * *caller never says which*. That is the whole point: a message about the
 * plate, the models, the slice or the printer belongs over the scene where the
 * work is happening, and it only falls back to the window when there is no
 * scene to put it over. Anything anchored to a control the user can see —
 * a failed import in a settings page, a caution about a setting — does not come
 * here at all; it uses `nexus-inline-notice` beside the control.
 *
 * A running job **resolves in place** ({@link resolveTask}) rather than handing
 * its result to a second surface in another corner. The progress you watched is
 * the thing that tells you how it went.
 */
@Injectable({ providedIn: 'root' })
export class NotificationService {
  /** Every live notice, in the order it arrived. */
  readonly notices = signal<Notice[]>([]);

  /** Those drawn over the scene. */
  readonly sceneNotices = computed(() => this.notices().filter((n) => n.placement === 'scene'));

  /** Those drawn in the window dock. */
  readonly windowNotices = computed(() => this.notices().filter((n) => n.placement === 'window'));

  /** The active full-page celebration, or `null` when none is playing. */
  readonly celebration = signal<Celebration | null>(null);

  /**
   * Announce that a scene strip is on screen, and can be spoken into.
   *
   * Called by the component that renders it; the returned function withdraws
   * the claim. Without this the service has no way to know that a {@link task}
   * raised during a cold launch — before the slice workspace exists — would be
   * drawn into a component that is not mounted, which is how "Opening model…"
   * used to be invisible and then report success out of nowhere.
   */
  registerSceneHost(): () => void {
    this.#sceneHosts.update((n) => n + 1);
    return () => this.#sceneHosts.update((n) => Math.max(0, n - 1));
  }

  /** Push a note and return its id. */
  note(tone: NoticeTone, title: string, options: NoticeOptions = {}): string {
    return this.#push({
      tone,
      title,
      message: options.message,
      icon: options.icon,
      action: options.action,
      onDismiss: options.onDismiss,
      progress: null,
      dismissible: options.dismissible ?? true,
      autoDismissMs:
        options.autoDismissMs === undefined
          ? options.action
            ? null
            : TONE_DISMISS_MS[tone]
          : options.autoDismissMs,
      placement: this.#placementFor(options.scope),
    });
  }

  info(title: string, message?: string, autoDismissMs?: number): string {
    return this.note('info', title, { message, autoDismissMs });
  }

  success(title: string, message?: string, autoDismissMs?: number): string {
    return this.note('success', title, { message, autoDismissMs });
  }

  warning(title: string, message?: string, autoDismissMs?: number): string {
    return this.note('warning', title, { message, autoDismissMs });
  }

  error(title: string, message?: string, autoDismissMs?: number): string {
    return this.note('danger', title, { message, autoDismissMs });
  }

  /**
   * Ask a question that waits. Stays until the action is taken or the user
   * dismisses it — dismissal *is* the second answer, so a prompt never needs a
   * second button.
   */
  prompt(
    title: string,
    action: NoticeAction,
    options: Omit<NoticeOptions, 'action' | 'autoDismissMs'> = {},
  ): string {
    return this.note('info', title, { ...options, action, autoDismissMs: null });
  }

  /**
   * Start a determinate job. Returns the id — call {@link updateTask}, then
   * {@link resolveTask} to finish it where it started.
   */
  task(title: string, message?: string): string {
    return this.#push({
      tone: 'info',
      title,
      message,
      progress: 0,
      dismissible: false,
      autoDismissMs: null,
      placement: this.#placementFor(),
    });
  }

  /** Update the progress (0–100) and optionally change the message. */
  updateTask(id: string, progress: number, message?: string): void {
    this.notices.update((list) =>
      list.map((n) =>
        n.id === id ? { ...n, progress, ...(message !== undefined ? { message } : {}) } : n,
      ),
    );
  }

  /**
   * Finish a job **in the notice it was already showing in**: the bar fills,
   * the tone changes, and it leaves on its own a few seconds later. Nothing is
   * handed to another surface, so the user's eye never has to find the result
   * somewhere other than where they were watching.
   */
  resolveTask(id: string, tone: NoticeTone, title: string, message?: string): void {
    const exists = this.notices().some((n) => n.id === id);
    if (!exists) {
      // The job outlived its notice (dismissed, or evicted by a burst). Say it
      // once, plainly, rather than dropping the outcome.
      this.note(tone, title, { message });
      return;
    }

    this.notices.update((list) =>
      list.map((n) =>
        n.id === id
          ? {
              ...n,
              tone,
              title,
              message,
              progress: 100,
              dismissible: true,
              autoDismissMs: RESOLVED_HOLD_MS[tone],
            }
          : n,
      ),
    );
    this.#scheduleAutoDismiss(id, RESOLVED_HOLD_MS[tone]);
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

  /** Put a notice away. For a prompt, that is also how the user declines it. */
  dismiss(id: string): void {
    const notice = this.notices().find((n) => n.id === id);
    this.#remove(id);
    notice?.onDismiss?.();
  }

  /** Run a notice's action, then retire it — the answer is given. */
  act(id: string): void {
    const notice = this.notices().find((n) => n.id === id);
    if (!notice?.action) {
      return;
    }
    this.#remove(id);
    notice.action.run();
  }

  /**
   * Hold the auto-dismiss countdown — while a notice is hovered or focused, so
   * a message cannot evaporate mid-sentence while it is being read.
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
    const remaining = this.notices().find((n) => n.id === id)?.autoDismissMs;
    if (remaining) {
      this.#scheduleAutoDismiss(id, remaining);
    }
  }

  readonly #timers = new Map<string, ReturnType<typeof setTimeout>>();
  readonly #sceneHosts = signal(0);
  readonly #viewport = inject(Viewport);

  #remove(id: string): void {
    this.pauseAutoDismiss(id);
    this.notices.update((list) => list.filter((n) => n.id !== id));
  }

  /**
   * Which dock a message lands in.
   *
   * Two docks exist because a desktop window has two places worth speaking
   * from. **A phone has one.** There the layout is already a single column —
   * the slice sheet owns the bottom and the tab bar owns what is under it —
   * and a second anchor at the top simply lands on the first. So on a handheld
   * the scene absorbs everything, `scope: 'app'` included: the reload prompt is
   * no less urgent than what the plate has to say, and there is nowhere else
   * for it to be.
   */
  #placementFor(scope?: 'app'): NoticePlacement {
    if (this.#sceneHosts() === 0) {
      return 'window';
    }
    return scope === 'app' && !this.#viewport.isHandheld() ? 'window' : 'scene';
  }

  #push(partial: Omit<Notice, 'id'>): string {
    const id = String(_nextId++);
    const notice: Notice = { id, ...partial };

    this.notices.update((list) => {
      // An identical message repeated (the same failure retried, the same file
      // re-imported) replaces its predecessor rather than stacking a duplicate.
      const deduped = list.filter(
        (n) => !(n.title === notice.title && n.message === notice.message),
      );
      // Cap each dock independently: a busy scene must not evict the app-level
      // notice sitting in the other one.
      //
      // A standing prompt — one that neither expires nor offers a close button,
      // like the reload prompt — is never a candidate. It has no way back once
      // it is gone, and the thing it is waiting for has not happened yet.
      const sameDock = deduped.filter(
        (n) => n.placement === notice.placement && (n.dismissible || n.autoDismissMs !== null),
      );
      const evicted = sameDock.slice(0, Math.max(0, sameDock.length + 1 - MAX_VISIBLE));
      for (const gone of evicted) {
        this.pauseAutoDismiss(gone.id);
      }
      return [...deduped.filter((n) => !evicted.includes(n)), notice];
    });

    if (notice.autoDismissMs) {
      this.#scheduleAutoDismiss(id, notice.autoDismissMs);
    }

    return id;
  }

  #scheduleAutoDismiss(id: string, delayMs: number): void {
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
