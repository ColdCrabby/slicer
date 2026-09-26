import { Injectable, computed, inject, signal } from '@angular/core';
import { BrowserStorage } from './browser-storage';

const PINNED_KEY = 'settings-nav.pinned';

/** The Settings section list's width when open, in px. Its stylesheet reads this. */
export const NAV_OPEN_WIDTH = 240;

/** Its width folded to an icon rail, in px. */
export const NAV_FOLDED_WIDTH = 56;

/**
 * Whether the Settings section list is folded down to an icon rail.
 *
 * A service rather than shell state because two unrelated surfaces decide it:
 * the user, with the fold button, and the profile editors, whose contents
 * outline needs a column of its own. Settings is three columns before the
 * outline asks for a fourth — sections, list, editor — and on a window the
 * width of an iPad held sideways the only one that can give up enough room is
 * the section list, whose labels an icon rail can stand in for.
 *
 * So the fold has two sources, and the user's wins:
 *
 * - **Pinned** — the user pressed the fold button. That choice is remembered
 *   and applies everywhere, and the page never overrides it: someone who opened
 *   the list on purpose has said they would rather read it than the outline.
 * - **Automatic** — until then, a profile editor folds the list while it needs
 *   the room ({@link requestFold}), and the list opens again when they leave.
 *   General, Appearance and the rest have nothing to make room for, so the
 *   labels are back the moment they are useful.
 */
@Injectable({ providedIn: 'root' })
export class SettingsNav {
  private readonly storage = inject(BrowserStorage);

  /** The user's own choice — `true` folded, `false` open — or `null` for automatic. */
  private readonly pinned = signal<boolean | null>(
    this.storage.getJson<boolean>(PINNED_KEY, 'local'),
  );

  /** Whether the page on screen has asked for the fold to make room. */
  private readonly foldRequested = signal(false);

  readonly collapsed = computed(() => this.pinned() ?? this.foldRequested());

  /**
   * Whether the list is folded because a page needed the room, rather than
   * because the user folded it. The toggle's label says so, so a list that
   * folded itself is never a mystery.
   */
  readonly foldedForRoom = computed(() => this.pinned() === null && this.foldRequested());

  toggle(): void {
    const next = !this.collapsed();
    this.pinned.set(next);
    this.storage.writeJson(PINNED_KEY, next, 'local');
  }

  /**
   * Ask for the list to fold (or stop asking). Ignored while the user has
   * pinned it either way. Every caller must withdraw the request when it goes
   * away, or the list would stay folded on a page with nothing to fold for.
   */
  requestFold(fold: boolean): void {
    this.foldRequested.set(fold);
  }
}
