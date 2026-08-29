import {
  afterRenderEffect,
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  inject,
  input,
} from '@angular/core';
import { MarkdownComponent } from 'ngx-markdown';
import type { ChangelogEntry } from '../../services/scene-engine';

/**
 * How long to keep correcting the reveal after the list first renders.
 *
 * Rendered markdown reflows the list *after* the initial paint (and each body
 * is far taller than its placeholder), so a single measurement taken on the
 * first frame reads stale offsets and lands short — or, worse, concludes the
 * target is already on screen when it is about to be pushed far below.
 */
const REVEAL_SETTLE_MS = 500;

/**
 * The full release history, newest first, with the running release called out.
 *
 * This is the *only* changelog renderer in the app: the "What's New" settings
 * section and the dialog shown after an upgrade both mount this component with
 * the same entries. Rather than showing a different (filtered) list after an
 * update, {@link currentVersion} is highlighted and scrolled into view, so the
 * two surfaces can never drift apart in look or content.
 *
 * Purely presentational — the caller supplies the parsed entries and decides
 * which version counts as "current" (a development build, for example, is on
 * `Unreleased`).
 */
@Component({
  selector: 'nexus-changelog-list',
  imports: [MarkdownComponent],
  templateUrl: './changelog-list.html',
  styleUrl: './changelog-list.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ChangelogList {
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  /** Parsed changelog sections, newest first. */
  readonly entries = input.required<ChangelogEntry[]>();

  /**
   * Version label to highlight and reveal, matched against
   * {@link ChangelogEntry.version} — typically the running release, or
   * `"Unreleased"` for a development build. `null` highlights nothing.
   */
  readonly currentVersion = input<string | null>(null);

  /** The version this instance already revealed, so it only happens once. */
  private revealedVersion: string | null = null;

  /** Set once the component is torn down, to stop the settle loop. */
  private destroyed = false;

  constructor() {
    inject(DestroyRef).onDestroy(() => (this.destroyed = true));

    afterRenderEffect({
      read: () => {
        const version = this.currentVersion();
        // Tracked so the reveal retries once the entries actually arrive.
        const hasEntries = this.entries().length > 0;
        if (!version || !hasEntries || this.revealedVersion === version) {
          return;
        }

        const el = this.host.nativeElement.querySelector<HTMLElement>('[data-current="true"]');
        if (!el) {
          return;
        }

        this.revealedVersion = version;
        this.revealWhenSettled(el);
      },
    });
  }

  /**
   * Bring `el` into view, re-checking until the list stops reflowing.
   *
   * Each pass is a no-op once the release heading is on screen, so this settles
   * instead of fighting the reader: content growing *above* the target pushes it
   * off screen again and gets corrected, content growing below never does.
   */
  private revealWhenSettled(el: HTMLElement): void {
    const deadline = performance.now() + REVEAL_SETTLE_MS;

    const tick = () => {
      if (this.destroyed || !el.isConnected) {
        return;
      }
      if (!isHeadingVisible(el)) {
        el.scrollIntoView({ block: 'start' });
      }
      if (performance.now() < deadline) {
        requestAnimationFrame(tick);
      }
    };

    requestAnimationFrame(tick);
  }
}

/**
 * Whether the top of `el` is already within its scrollport.
 *
 * A release section is usually taller than the view, so `scrollIntoView` would
 * pin its top edge to the scrollport even when it is the very first thing on
 * screen — hiding the page heading above it for no benefit. Scrolling is only
 * worth it when the reader cannot see where the release starts.
 */
function isHeadingVisible(el: HTMLElement): boolean {
  const top = el.getBoundingClientRect().top;
  const scroller = findScrollParent(el);
  if (!scroller) {
    return top >= 0 && top < window.innerHeight;
  }
  const bounds = scroller.getBoundingClientRect();
  return top >= bounds.top && top < bounds.bottom;
}

/** Nearest ancestor that actually scrolls vertically, or `null` for the page. */
function findScrollParent(el: HTMLElement): HTMLElement | null {
  for (let node = el.parentElement; node; node = node.parentElement) {
    const overflowY = getComputedStyle(node).overflowY;
    if ((overflowY === 'auto' || overflowY === 'scroll') && node.scrollHeight > node.clientHeight) {
      return node;
    }
  }
  return null;
}
