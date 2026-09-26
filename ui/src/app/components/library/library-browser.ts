import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  computed,
  effect,
  inject,
  input,
  linkedSignal,
  output,
  signal,
  viewChild,
} from '@angular/core';
import { Router } from '@angular/router';
import { Icon } from '@coldcrabby/ui';
import type { ContextMenuItem } from '../../services/context-menu/context-menu.model';
import { ContextMenuService } from '../../services/context-menu/context-menu.service';
import { ContextMenuTrigger } from '../../services/context-menu/context-menu-trigger';
import { KeyboardShortcuts } from '../../services/keyboard-shortcuts/keyboard-shortcuts';
import { ObjectLibrary, type LibraryEntry } from '../../services/library';
import { LibraryActions } from '../../services/library/library-actions';
import { LibraryCard } from './library-card';

type SortOrder = 'recent' | 'added' | 'name' | 'used';

const SORT_LABELS: Record<SortOrder, string> = {
  recent: 'Recently used',
  added: 'Recently added',
  used: 'Most used',
  name: 'Name',
};

const SORT_KEY = 'nexus.library.sort';

/**
 * How many cards join the grid each time its end scrolls into view.
 *
 * Infinite scroll rather than pages: the library is browsed by eye, and a page
 * boundary splits what the user is scanning for no reason they can see. The
 * entries themselves are a few hundred bytes each and already in memory — what
 * is worth pacing is the DOM, and each card's thumbnail, which is only fetched
 * once the card is near the screen.
 */
const BATCH = 60;

/** Without an observer to say when the end is near, everything is drawn. */
const FIRST_BATCH = typeof IntersectionObserver === 'undefined' ? Number.MAX_SAFE_INTEGER : BATCH;

/**
 * The library as a searchable, sortable grid — shared by the full page and the
 * flyout beside the plate.
 *
 * It owns everything that is the same in both: finding a model, the grid, and
 * each model's context menu. What a click *means* is the host's call — the page
 * selects, the flyout adds to the plate — so a click is only reported.
 */
@Component({
  selector: 'nexus-library-browser',
  imports: [ContextMenuTrigger, Icon, LibraryCard],
  templateUrl: './library-browser.html',
  styleUrl: './library-browser.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[class.is-flyout]': "variant() === 'flyout'" },
})
export class LibraryBrowser {
  protected readonly library = inject(ObjectLibrary);
  protected readonly actions = inject(LibraryActions);
  readonly #menu = inject(ContextMenuService);
  readonly #router = inject(Router);
  readonly #shortcuts = inject(KeyboardShortcuts);
  private readonly searchInput = viewChild.required<ElementRef<HTMLInputElement>>('search');

  /** `page` selects on click; `flyout` sits beside a plate and is narrower. */
  readonly variant = input<'page' | 'flyout'>('page');
  readonly selectedId = input<string | null>(null);

  /** A card was clicked or tapped. */
  readonly pick = output<LibraryEntry>();
  /** A card was opened — double-click or Enter. */
  readonly activate = output<LibraryEntry>();
  /** Rename was chosen from a card's menu. */
  readonly rename = output<LibraryEntry>();

  protected readonly query = signal('');
  protected readonly sort = signal<SortOrder>(readSort());
  protected readonly sortLabel = computed(() => SORT_LABELS[this.sort()]);

  protected readonly entries = computed(() => {
    const query = this.query().trim().toLowerCase();
    const filtered = this.library
      .entries()
      .filter((e) => !query || e.name.toLowerCase().includes(query));
    const sort = this.sort();
    return [...filtered].sort((a, b) => {
      switch (sort) {
        case 'name':
          return a.name.localeCompare(b.name, undefined, { numeric: true });
        case 'added':
          return b.added_at.localeCompare(a.added_at);
        case 'used':
          return (b.use_count ?? 0) - (a.use_count ?? 0) || a.name.localeCompare(b.name);
        default:
          return (b.last_used_at ?? b.added_at).localeCompare(a.last_used_at ?? a.added_at);
      }
    });
  });

  /** How far down the grid is drawn; back to one batch whenever the list changes. */
  readonly #limit = linkedSignal({
    source: () => [this.query(), this.sort()] as const,
    computation: () => FIRST_BATCH,
  });
  protected readonly shown = computed(() => this.entries().slice(0, this.#limit()));
  protected readonly more = computed(() => this.entries().length > this.#limit());

  private readonly sentinel = viewChild<ElementRef<HTMLElement>>('sentinel');

  constructor() {
    // ⌘F / Ctrl+F lands in this grid's search while it is on screen.
    const searchRef = { focusSearch: () => this.searchInput().nativeElement.select() };
    this.#shortcuts.librarySearchRef = searchRef;
    inject(DestroyRef).onDestroy(() => {
      if (this.#shortcuts.librarySearchRef === searchRef) {
        this.#shortcuts.librarySearchRef = null;
      }
    });

    if (typeof IntersectionObserver === 'undefined') {
      return;
    }
    const observer = new IntersectionObserver(
      (records) => {
        if (records.some((r) => r.isIntersecting)) {
          this.#limit.update((n) => n + BATCH);
        }
      },
      { rootMargin: '400px' },
    );
    // Re-observed after every batch as well as whenever the sentinel comes
    // back: observing fires once with the current state, so a sentinel still
    // on screen after a batch — a tall window — asks for the next one.
    effect((onCleanup) => {
      const el = this.sentinel()?.nativeElement;
      this.shown();
      if (el) {
        observer.observe(el);
        onCleanup(() => observer.unobserve(el));
      }
    });
    inject(DestroyRef).onDestroy(() => observer.disconnect());
  }

  /**
   * Arrow keys walk the grid as it is laid out — left and right along a row,
   * up and down by however many columns the width allows — with Home and End
   * for the ends. Moving is not choosing: Space selects and Enter adds, as the
   * card says, so walking through a flyout never puts anything on the plate.
   * The grid is a listbox, which the plate's nudge keys already leave alone.
   */
  protected onGridKey(event: KeyboardEvent, grid: HTMLElement): void {
    const cards = Array.from(grid.querySelectorAll<HTMLElement>('nexus-library-card'));
    const at = cards.indexOf(document.activeElement as HTMLElement);
    if (at < 0 || cards.length === 0) {
      return;
    }
    const columns = getComputedStyle(grid).gridTemplateColumns.split(' ').length || 1;
    const step: Record<string, number> = {
      ArrowLeft: -1,
      ArrowRight: 1,
      ArrowUp: -columns,
      ArrowDown: columns,
      Home: -at,
      End: cards.length - 1 - at,
    };
    if (!(event.key in step)) {
      return;
    }
    event.preventDefault();
    const next = Math.min(cards.length - 1, Math.max(0, at + step[event.key]));
    cards[next].focus();
    cards[next].scrollIntoView({ block: 'nearest' });
  }

  /** Down from the search field drops into the results, as in any finder. */
  protected focusFirstCard(grid: HTMLElement): void {
    grid.querySelector<HTMLElement>('nexus-library-card')?.focus();
  }

  protected clearQuery(input: HTMLInputElement): void {
    this.query.set('');
    input.focus();
  }

  /** Escape empties the search first; only an empty one lets Escape through. */
  protected escapeSearch(event: Event, input: HTMLInputElement): void {
    if (this.query()) {
      event.stopPropagation();
      this.clearQuery(input);
    }
  }

  protected openSortMenu(button: HTMLElement): void {
    const orders: SortOrder[] = ['recent', 'added', 'used', 'name'];
    void this.#menu.open(
      anchorBelow(button),
      orders.map((order) => ({
        label: SORT_LABELS[order],
        checked: this.sort() === order,
        action: () => {
          this.sort.set(order);
          try {
            localStorage.setItem(SORT_KEY, order);
          } catch {
            // A remembered sort is a nicety; private windows may refuse it.
          }
        },
      })),
    );
  }

  protected openCardMenu(event: MouseEvent, entry: LibraryEntry): void {
    void this.#menu.open(event, this.#cardMenu(entry));
  }

  #cardMenu(entry: LibraryEntry): ContextMenuItem[] {
    const available = entry.locations?.some((l) => !l.missing) ?? false;
    // Beside a plate the flyout knows which workplate is meant; on the page the
    // user chooses one.
    const items: ContextMenuItem[] =
      this.variant() === 'flyout'
        ? [
            {
              label: 'Add to workplate',
              icon: 'plus',
              disabled: !available,
              action: () => void this.actions.addToPlate(entry),
            },
          ]
        : [
            {
              label: 'Add to workplate',
              icon: 'plus',
              disabled: !available || this.actions.workplates().length === 0,
              submenu: this.actions.workplateMenu(entry, { withNew: false }),
            },
          ];
    items.push({
      label: 'New workplate',
      icon: 'page-plus',
      disabled: !available,
      action: () => void this.actions.openAsPlate(entry),
    });
    items.push({ label: '', separator: true });
    if (this.variant() === 'page') {
      items.push({ label: 'Rename', icon: 'edit-pencil', action: () => this.rename.emit(entry) });
    } else {
      items.push({
        label: 'Show in library',
        icon: 'book-stack',
        action: () =>
          void this.#router.navigate(['/library'], { queryParams: { model: entry.id } }),
      });
    }
    if (this.library.capabilities.reveal && available) {
      items.push({
        label: revealLabel(),
        icon: 'folder',
        action: () => void this.library.reveal(entry),
      });
    }
    items.push(
      { label: '', separator: true },
      {
        // A submenu is the confirm step: removing takes a second, deliberate
        // choice, which is what the inline two-step is everywhere else.
        label: removeLabel(entry),
        icon: 'trash',
        danger: true,
        submenu: [
          {
            label: `Remove “${entry.name}”`,
            danger: true,
            action: () => void this.library.remove(entry),
          },
        ],
      },
    );
    return items;
  }
}

/** A menu event anchored under a button, for menus opened by click or keyboard. */
function anchorBelow(el: HTMLElement): MouseEvent {
  const rect = el.getBoundingClientRect();
  return new MouseEvent('contextmenu', { clientX: rect.left, clientY: rect.bottom + 4 });
}

function readSort(): SortOrder {
  try {
    const stored = localStorage.getItem(SORT_KEY);
    if (stored && stored in SORT_LABELS) {
      return stored as SortOrder;
    }
  } catch {
    // Unreadable storage just means the default order.
  }
  return 'recent';
}

/** What the system file manager is called here. */
export function revealLabel(): string {
  const platform = globalThis.navigator?.userAgent ?? '';
  if (/Mac/i.test(platform)) return 'Show in Finder';
  if (/Win/i.test(platform)) return 'Show in Explorer';
  return 'Show in folder';
}

/** Removing an entry the library holds a copy of deletes that copy. */
export function removeLabel(entry: LibraryEntry): string {
  return entry.locations?.some((l) => l.kind === 'copy')
    ? 'Remove from library'
    : 'Forget this model';
}
