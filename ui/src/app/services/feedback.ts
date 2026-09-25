import { inject, Injectable } from '@angular/core';
import { isTauriHost, resolveRuntimeMode } from '../runtime/domain/runtime-mode.util';
import { AppVersion } from './app-version';
import { BrowserStorage } from './browser-storage';
import { NotificationService } from './notifications';

/** Where feedback lands: a new issue on the public tracker. */
const NEW_ISSUE_URL = 'https://github.com/ColdCrabby/slicer/issues/new';

/** Successful slices counted across visits. */
const SLICE_COUNT_KEY = 'slicer:feedback-slice-count';

/** How many times the prompt has been shown. */
const PROMPTS_SHOWN_KEY = 'slicer:feedback-prompts-shown';

/** Set once the user has opened the feedback form, from anywhere. */
const FEEDBACK_GIVEN_KEY = 'slicer:feedback-given';

/**
 * The slice counts at which to ask — and so, at most, how often.
 *
 * The first comes once the user has sliced enough to have an opinion but is
 * still new enough to remember what tripped them up. The second is for
 * someone who has kept coming back; after that the app never asks again.
 */
const PROMPT_AT_SLICES = [3, 30] as const;

/**
 * Build the new-issue link with the build and runtime already filled in, so
 * the first thing a maintainer asks is already answered and the user only
 * writes the part only they know.
 */
export function feedbackUrl(version: string | null, runtime: string, userAgent: string): string {
  const body = [
    '<!-- What worked, what did not, what you wish it did. Screenshots welcome. -->',
    '',
    '',
    '---',
    `Version: ${version ?? 'unknown'}`,
    `Runtime: ${runtime}`,
    `Browser: ${userAgent}`,
  ].join('\n');
  return `${NEW_ISSUE_URL}?${new URLSearchParams({ body }).toString()}`;
}

/**
 * The one way into sending feedback, and the rule for when the app offers it.
 *
 * Present rather than pushy: the help menu always carries a way in, and a
 * notice over the plate offers it twice in the app's life, right after a slice
 * succeeds — a moment when the user is waiting on nothing. Taking it once, from
 * either place, retires the notice for good.
 */
@Injectable({ providedIn: 'root' })
export class Feedback {
  private readonly storage = inject(BrowserStorage);
  private readonly notifications = inject(NotificationService);
  private readonly appVersion = inject(AppVersion);

  /** Open the feedback form outside the app. */
  open(): void {
    this.storage.write(FEEDBACK_GIVEN_KEY, '1');
    const url = feedbackUrl(
      this.appVersion.info()?.git_describe ?? null,
      resolveRuntimeMode(),
      typeof navigator === 'undefined' ? 'unknown' : navigator.userAgent,
    );
    if (isTauriHost()) {
      void import('@tauri-apps/plugin-shell').then(({ open }) => open(url));
      return;
    }
    window.open(url, '_blank', 'noopener,noreferrer');
  }

  /** Call on every successful slice; offers feedback when a threshold is reached. */
  recordSlice(): void {
    const slices = Number(this.storage.get(SLICE_COUNT_KEY)() ?? 0) + 1;
    this.storage.write(SLICE_COUNT_KEY, String(slices));
    if (this.storage.get(FEEDBACK_GIVEN_KEY)()) {
      return;
    }
    const shown = Number(this.storage.get(PROMPTS_SHOWN_KEY)() ?? 0);
    const next = PROMPT_AT_SLICES[shown];
    if (next === undefined || slices < next) {
      return;
    }
    this.storage.write(PROMPTS_SHOWN_KEY, String(shown + 1));

    this.notifications.note('info', 'How is slicing going?', {
      message: 'A line about what worked or got in your way shapes what gets built next.',
      icon: 'chat-lines',
      autoDismissMs: null,
      action: { label: 'Send feedback', run: () => this.open() },
    });
  }
}
