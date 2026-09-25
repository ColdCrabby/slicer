import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  afterNextRender,
  computed,
  input,
  output,
  signal,
  viewChild,
  viewChildren,
} from '@angular/core';
import { Icon, IconButton } from '@coldcrabby/ui';

/** One row of the tab search: an open tab reduced to what the list shows. */
export interface TabSearchEntry {
  uuid: string;
  /** What the tab strip shows — the custom name, or the derived one. */
  name: string;
  /** The model's filename, or `null` where it would only repeat {@link name}. */
  filename: string | null;
}

/**
 * The "search tabs" list: every open workplate, filtered as you type.
 *
 * Presentational only — the tab strip owns the list, switching and closing,
 * and this renders what it is given. The keyboard never leaves the search box:
 * the arrows move a highlight through the list and `Enter` takes it, the way a
 * browser's own tab search behaves, so the list rows need no focus of their own.
 */
@Component({
  selector: 'nexus-workplate-tab-search',
  imports: [Icon, IconButton],
  templateUrl: './workplate-tab-search.html',
  styleUrl: './workplate-tab-search.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WorkplateTabSearch {
  readonly entries = input.required<readonly TabSearchEntry[]>();
  readonly activeUuid = input<string | null>(null);

  readonly pick = output<string>();
  readonly closeTab = output<string>();
  readonly dismiss = output<void>();

  protected readonly query = signal('');
  /** Index into {@link filtered} of the highlighted row. */
  protected readonly highlighted = signal(0);

  private readonly search = viewChild.required<ElementRef<HTMLInputElement>>('search');
  private readonly rows = viewChildren<ElementRef<HTMLElement>>('row');

  protected readonly filtered = computed(() => {
    const terms = this.query().trim().toLowerCase().split(/\s+/).filter(Boolean);
    const entries = this.entries();
    if (terms.length === 0) {
      return entries;
    }
    return entries.filter((entry) => {
      const haystack = `${entry.name} ${entry.filename ?? ''}`.toLowerCase();
      return terms.every((term) => haystack.includes(term));
    });
  });

  constructor() {
    // Opened by a click or a shortcut, the list is only useful once you can
    // type into it — so the box takes focus the moment it is on screen.
    afterNextRender(() => this.search().nativeElement.focus());
  }

  protected onInput(event: Event): void {
    this.query.set((event.target as HTMLInputElement).value);
    this.highlighted.set(0);
  }

  protected onKeydown(event: KeyboardEvent): void {
    const count = this.filtered().length;
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        this.moveHighlight(count ? (this.highlighted() + 1) % count : 0);
        break;
      case 'ArrowUp':
        event.preventDefault();
        this.moveHighlight(count ? (this.highlighted() - 1 + count) % count : 0);
        break;
      case 'Enter': {
        event.preventDefault();
        const entry = this.filtered()[this.highlighted()];
        if (entry) {
          this.pick.emit(entry.uuid);
        }
        break;
      }
      case 'Escape':
        event.preventDefault();
        // Stop here, or the global Escape shortcut also clears the scene
        // selection behind a list the user was only dismissing.
        event.stopPropagation();
        this.dismiss.emit();
        break;
    }
  }

  protected onClose(uuid: string, event: Event): void {
    event.stopPropagation();
    this.closeTab.emit(uuid);
    // The row under the pointer is gone; keep the highlight inside the list.
    this.highlighted.update((i) => Math.min(i, Math.max(0, this.filtered().length - 2)));
    this.search().nativeElement.focus();
  }

  private moveHighlight(index: number): void {
    this.highlighted.set(index);
    this.rows()[index]?.nativeElement.scrollIntoView({ block: 'nearest' });
  }
}
