import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { DangerZone } from '../../services/danger-zone';
import { Button, InlineNotice, type InlineNoticeTone, SectionHeader } from '@coldcrabby/ui';

/** The word the user must type to unlock an irreversible reset. */
const CONFIRM_WORD = 'RESET';

/** How long the inline two-step confirm stays armed before reverting. */
const CONFIRM_TIMEOUT_MS = 4000;

/** Which typed-challenge action, if any, is currently expanded. */
type Challenge = 'profiles' | 'factory' | null;

/**
 * Settings "Danger Zone": irreversible maintenance actions (clear slice
 * history, reset profiles, factory reset). Each action confirms by impact —
 * the routine one (clear history) uses an inline two-step confirm; the
 * data-losing ones require typing {@link CONFIRM_WORD}. All wiring lives in
 * {@link DangerZone}, which routes to the right place per runtime.
 */
@Component({
  selector: 'nexus-settings-danger-zone',
  imports: [FormsModule, Button, InlineNotice, SectionHeader],
  templateUrl: './danger-zone.html',
  styleUrl: './danger-zone.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DangerZoneSettings {
  private readonly danger = inject(DangerZone);

  protected readonly confirmWord = CONFIRM_WORD;
  protected readonly canClearHistory = this.danger.canClearHistory;
  protected readonly storageScope = this.danger.storageScope;

  /** True while the clear-history button is armed for its second click. */
  protected readonly historyArmed = signal(false);
  /** True while an action is running, to disable the controls. */
  protected readonly busy = signal(false);

  /** Which typed-challenge panel is open, plus the text typed so far. */
  protected readonly challenge = signal<Challenge>(null);
  protected readonly typed = signal('');
  protected readonly challengeMatches = computed(
    () => this.typed().trim().toUpperCase() === CONFIRM_WORD,
  );

  /**
   * What happened to the last clear, and why a reset would not run — both
   * shown beside the button that was pressed.
   *
   * These used to float in the corner of the window, which is the one place
   * the user was not looking: they had just armed a two-step confirm on a
   * specific card and were watching it. An outcome that belongs to a visible
   * control stays with that control.
   */
  protected readonly historyResult = signal<{ tone: InlineNoticeTone; text: string } | null>(null);
  protected readonly resetError = signal<string | null>(null);

  private historyTimer: ReturnType<typeof setTimeout> | null = null;

  /** First click arms; second click within the window clears the history. */
  protected async onClearHistory(): Promise<void> {
    if (this.busy()) {
      return;
    }
    if (!this.historyArmed()) {
      this.armHistory();
      return;
    }
    this.disarmHistory();
    this.busy.set(true);
    this.historyResult.set(null);
    try {
      await this.danger.clearHistory();
      this.historyResult.set({
        tone: 'info',
        text: 'Slice history and cached G-code were removed.',
      });
    } catch (error) {
      this.historyResult.set({ tone: 'danger', text: this.messageOf(error) });
    } finally {
      this.busy.set(false);
    }
  }

  /** Reset the two-step confirm if the button loses focus before the second click. */
  protected onHistoryBlur(): void {
    this.disarmHistory();
  }

  /** Open (or toggle) a typed-challenge panel, resetting any typed text. */
  protected openChallenge(which: Exclude<Challenge, null>): void {
    this.disarmHistory();
    this.typed.set('');
    this.resetError.set(null);
    this.challenge.set(this.challenge() === which ? null : which);
  }

  protected cancelChallenge(): void {
    this.challenge.set(null);
    this.typed.set('');
    this.resetError.set(null);
  }

  /** Run the confirmed reset. Both paths reload the page on success. */
  protected async confirmChallenge(): Promise<void> {
    if (!this.challengeMatches() || this.busy()) {
      return;
    }
    const which = this.challenge();
    this.busy.set(true);
    this.resetError.set(null);
    try {
      if (which === 'profiles') {
        await this.danger.resetProfiles();
      } else if (which === 'factory') {
        await this.danger.factoryReset();
      }
      // Both actions reload; nothing else to do here.
    } catch (error) {
      this.busy.set(false);
      this.resetError.set(this.messageOf(error));
    }
  }

  private armHistory(): void {
    // Only one destructive control is armed at a time.
    this.cancelChallenge();
    this.historyArmed.set(true);
    this.historyTimer = setTimeout(() => this.disarmHistory(), CONFIRM_TIMEOUT_MS);
  }

  private disarmHistory(): void {
    if (this.historyTimer) {
      clearTimeout(this.historyTimer);
      this.historyTimer = null;
    }
    this.historyArmed.set(false);
  }

  private messageOf(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
  }
}
