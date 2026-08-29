import { Injectable, Type, signal } from '@angular/core';
import { Observable, Subject, from } from 'rxjs';
import { isTauriMobile } from '../runtime/domain/runtime-mode.util';

export type DialogType = 'default' | 'warning' | 'danger';

export interface DialogConfig {
  title: string;
  message?: string;
  type?: DialogType;
  confirmLabel?: string;
  cancelLabel?: string;
  /** When true, only a single "OK" button is shown. */
  alertOnly?: boolean;
  /** Optional component rendered as the dialog body below the message. */
  content?: Type<unknown>;
  /** Optional preferred width for the dialog (e.g. '600px'). Capped by the default max-width rule. */
  preferredWidth?: string;
}

export interface ActiveDialog {
  id: string;
  config: DialogConfig;
  isLeaving: boolean;
  /** Resolves with true (confirm) or false (cancel / backdrop click). */
  result$: Subject<boolean>;
}

/** Duration (ms) of the leave animation — must match CSS. */
const LEAVE_DURATION_MS = 200;

let _nextId = 1;

@Injectable({ providedIn: 'root' })
export class Dialog {
  readonly activeDialog = signal<ActiveDialog | null>(null);

  /**
   * Open a confirmation dialog.
   * The returned Observable emits `true` when confirmed or `false` when
   * cancelled, then completes.
   */
  confirm(config: DialogConfig): Observable<boolean> {
    return this.#open(config);
  }

  /**
   * Open an alert dialog (single "OK" button).
   * The returned Observable emits once when dismissed, then completes.
   */
  alert(config: Omit<DialogConfig, 'cancelLabel' | 'alertOnly'>): Observable<boolean> {
    return this.#open({ ...config, alertOnly: true });
  }

  /** Called by the outlet to resolve and close the current dialog. */
  resolve(confirmed: boolean): void {
    const dialog = this.activeDialog();
    if (!dialog) {
      return;
    }

    const dialogId = dialog.id;

    // Start leave animation, then emit and clear.
    this.activeDialog.set({ ...dialog, isLeaving: true });

    setTimeout(() => {
      dialog.result$.next(confirmed);
      dialog.result$.complete();
      // Only clear if no new dialog was opened while the leave animation ran.
      if (this.activeDialog()?.id === dialogId) {
        this.activeDialog.set(null);
      }
    }, LEAVE_DURATION_MS);
  }

  #open(config: DialogConfig): Observable<boolean> {
    // iOS draws its own alerts. A dialog that embeds a component still needs
    // the HTML implementation — UIKit alerts take a title and message, nothing
    // richer — so only plain confirm/alert dialogs are handed to the platform.
    if (isTauriMobile() && !config.content) {
      return from(this.#openNative(config));
    }

    // Resolve any existing dialog as cancelled before showing the new one.
    const existing = this.activeDialog();
    if (existing) {
      existing.result$.next(false);
      existing.result$.complete();
    }

    const result$ = new Subject<boolean>();
    const dialog: ActiveDialog = {
      id: String(_nextId++),
      config,
      isLeaving: false,
      result$,
    };

    this.activeDialog.set(dialog);
    return result$.asObservable();
  }

  /**
   * Present the request as a real `UIAlertController`.
   *
   * Falls back to the HTML dialog if the command is unavailable, so a missing
   * or failed native path degrades instead of losing the prompt entirely.
   */
  async #openNative(config: DialogConfig): Promise<boolean> {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      return await invoke<boolean>('show_native_dialog', {
        request: {
          title: config.title,
          message: config.message ?? null,
          confirm_label: config.confirmLabel ?? 'OK',
          // `alertOnly` means a single OK button, which UIKit expresses by
          // simply having no cancel action.
          cancel_label: config.alertOnly ? null : (config.cancelLabel ?? 'Cancel'),
          destructive: config.type === 'danger',
        },
      });
    } catch {
      return await this.#openHtmlFallback(config);
    }
  }

  #openHtmlFallback(config: DialogConfig): Promise<boolean> {
    const result$ = new Subject<boolean>();
    this.activeDialog.set({
      id: String(_nextId++),
      config,
      isLeaving: false,
      result$,
    });
    return new Promise((resolve) => result$.subscribe((value) => resolve(value)));
  }
}
