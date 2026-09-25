import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  computed,
  inject,
  input,
  output,
} from '@angular/core';
import { Icon } from '@coldcrabby/ui';
import type { LibraryEntry } from '../../services/library';

/**
 * One model in the library grid: its picture, its name, and whether it can
 * still be read.
 *
 * Presentational. It says once when it first scrolls into view — that is when
 * the page asks for its thumbnail, so a library of hundreds only draws the
 * pictures someone actually looks at.
 */
@Component({
  selector: 'nexus-library-card',
  imports: [Icon],
  templateUrl: './library-card.html',
  styleUrl: './library-card.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    role: 'option',
    '[attr.aria-selected]': 'selected()',
    '[class.is-selected]': 'selected()',
    '[class.is-missing]': 'missing()',
    tabindex: '0',
    '(click)': 'pick.emit()',
    '(dblclick)': 'activate.emit()',
    '(keydown.enter)': 'activate.emit()',
    '(keydown.space)': '$event.preventDefault(); pick.emit()',
  },
})
export class LibraryCard {
  readonly entry = input.required<LibraryEntry>();
  readonly thumbnail = input<string | null>(null);
  readonly rendering = input(false);
  readonly selected = input(false);

  /** Selected. */
  readonly pick = output<void>();
  /** Opened — double-click or Enter. */
  readonly activate = output<void>();
  /** First scrolled into view. */
  readonly visible = output<void>();

  protected readonly missing = computed(
    () => !(this.entry().locations ?? []).some((l) => !l.missing),
  );

  protected readonly meta = computed(() => {
    const entry = this.entry();
    const extents = entry.shape?.extents_mm;
    const size = extents ? `${extents.map((v) => Math.round(v)).join(' × ')} mm` : null;
    return [entry.format.toUpperCase(), size].filter(Boolean).join(' · ');
  });

  constructor() {
    const host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
    if (typeof IntersectionObserver === 'undefined') {
      queueMicrotask(() => this.visible.emit());
      return;
    }
    const observer = new IntersectionObserver(
      (records) => {
        if (records.some((r) => r.isIntersecting)) {
          observer.disconnect();
          this.visible.emit();
        }
      },
      { rootMargin: '200px' },
    );
    observer.observe(host);
    inject(DestroyRef).onDestroy(() => observer.disconnect());
  }
}
