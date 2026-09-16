import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  computed,
  effect,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Icon } from '@coldcrabby/ui';
import { Viewport } from '../../services/viewport';
import { KeyboardShortcuts } from '../../services/keyboard-shortcuts/keyboard-shortcuts';
import {
  filterOutline,
  hasRoomForRail,
  idsInView,
  measureOutline,
  scanOutline,
  type OutlineSection,
  type OutlineSpan,
} from './outline';

/** How long the landing mark on a jumped-to row lasts; matches `configure-flash`. */
const FLASH_MS = 1600;

/**
 * How long the editor must sit still before the outline re-reads it.
 *
 * Typing in a field mutates the subtree on every keystroke. A frame-coalesced
 * rescan still walked a two-hundred-row form between keystrokes, which is
 * precisely the cost a contents list is not allowed to add to typing; waiting
 * for a pause costs nothing the user can perceive, because the outline only has
 * to be right by the time they look at it.
 */
const RESCAN_QUIET_MS = 200;

/**
 * The contents rail beside a profile editor: every section of the page, and
 * under each one every setting by name, with a filter box above it.
 *
 * It reads the editor beside it rather than being told what is on it — see
 * `./outline.ts` for why — which is what lets one component serve the printer,
 * filament and print-profile pages without any of them describing themselves
 * twice. Drop it in as a sibling of `.mgr__detail` and it wires itself up.
 */
@Component({
  selector: 'nexus-profile-outline',
  standalone: true,
  imports: [FormsModule, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './profile-outline.html',
  styleUrl: './profile-outline.scss',
  host: {
    // Drives both its own `display` and the grid track the page reserves for
    // it, so a hidden rail costs no column.
    '[class.is-off]': '!visible()',
  },
})
export class ProfileOutline {
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly shortcuts = inject(KeyboardShortcuts);
  private readonly viewport = inject(Viewport);

  /**
   * The rail appears when there is genuinely room for it.
   *
   * It used to be tied to folding the Settings section list — a trade the user
   * made deliberately, because a fourth column at once made the page feel
   * crowded. The page has since given a column back, so the trade is no longer
   * the real question; the room is. Asking it directly also answers something
   * neither a viewport media query nor a folded-nav flag could: the nav, the
   * window and the dragged list width all take from the same budget, and only a
   * measurement of what is left knows about all three.
   *
   * Measured on [`.mgr__body`], whose own width does not depend on whether the
   * rail is showing — so revealing the rail can never be what takes the room
   * away that revealed it.
   */
  protected readonly visible = computed(() => this.roomForRail());
  private readonly roomForRail = signal(false);

  protected readonly query = signal('');

  /**
   * Sections the user has opened. Empty by default — every section starts
   * folded.
   *
   * A printer's editor runs to sixty settings and a print profile past two
   * hundred; listing all of them at once produces a rail as long as the page it
   * is meant to summarise, which is no longer a map. Folded, the rail is a dozen
   * lines and the whole editor fits on screen at once.
   */
  private readonly expanded = signal<ReadonlySet<string>>(new Set());

  /**
   * Whether a section's settings are listed.
   *
   * **A search ignores the folding entirely.** Someone typing a setting's name
   * is asking where it is, and answering with a collapsed section they must
   * then open is refusing to answer. Folding is for reading the outline, not
   * for searching it.
   */
  protected isExpanded(id: string): boolean {
    return !!this.query().trim() || this.expanded().has(id);
  }

  protected toggle(id: string): void {
    this.expanded.update((current) => {
      const next = new Set(current);
      if (!next.delete(id)) {
        next.add(id);
      }
      return next;
    });
  }

  /** Whether the bulk control currently offers to collapse rather than expand. */
  protected readonly anyExpanded = computed(() => this.expanded().size > 0);

  protected toggleAll(): void {
    this.expanded.set(
      this.anyExpanded() ? new Set() : new Set(this.sections().map((section) => section.id)),
    );
  }

  /** Go to a section and open it, since arriving somewhere folded is no arrival. */
  protected reveal(section: OutlineSection): void {
    this.expanded.update((current) => new Set(current).add(section.id));
    this.jump(section.el);
  }

  /**
   * Placeholder for the filter box, carrying the shortcut where there is a
   * keyboard to press it — the same judgement the slice sidebar's search makes.
   */
  protected readonly filterPlaceholder = computed(() =>
    this.viewport.isHandheld()
      ? 'Filter settings'
      : `Filter settings (${this.shortcuts.shortcutFor('focus-settings-search')})`,
  );

  /** Put the cursor in the filter box — the `$mod+f` the slice sidebar uses. */
  focusSearch(): void {
    this.searchInputRef()?.nativeElement.focus({ preventScroll: true });
  }

  /** The editor as it currently stands, rescanned whenever it changes. */
  private readonly sections = signal<OutlineSection[]>([]);

  /** The sections actually listed, narrowed by the filter box. */
  protected readonly rows = computed(() => filterOutline(this.sections(), this.query()));

  /** Section the editor is scrolled to, so the rail can say "you are here". */
  protected readonly currentId = signal<string | null>(null);

  /**
   * Every row whose target is on screen right now — not just the one at the
   * top.
   *
   * A contents list that marks a single active heading tells you where you are
   * and nothing about how much you can see; on an editor where a short section
   * fits entirely in the window, it also keeps pointing at the heading above
   * the thing you are reading. Lighting the whole visible span turns the rail
   * into a map of the page with your window drawn on it, which is what a reader
   * actually wants from one.
   *
   * A row half-cut by the edge of the editor does not count — see `idsInView`.
   */
  protected readonly inView = signal<ReadonlySet<string>>(new Set());

  private scroller: HTMLElement | null = null;

  /** Row geometry, measured per scan; see `measureOutline`. */
  private spans: ReadonlyMap<string, OutlineSpan> = new Map();

  /** Content height the spans were measured against, to notice a reflow. */
  private measuredHeight = 0;

  private readonly searchInputRef = viewChild<ElementRef<HTMLInputElement>>('searchInput');

  constructor() {
    // The same `$mod+f` the slice sidebar's settings search claims. The two are
    // never on screen together — one is the slice page, the other the settings
    // pages — so whichever is mounted answers it.
    this.shortcuts.settingsSearchRef = this;
    inject(DestroyRef).onDestroy(() => {
      if (this.shortcuts.settingsSearchRef === this) {
        this.shortcuts.settingsSearchRef = null;
      }
    });
    afterNextRender(() => this.attach());
    // Unfolding the rail has to read an editor the hidden rail never scanned.
    effect(() => {
      if (this.visible()) {
        this.scheduleRescan();
      }
    });
    inject(DestroyRef).onDestroy(() => this.detach());
  }

  // --- Wiring ------------------------------------------------------------

  private observer: MutationObserver | null = null;
  private roomObserver: ResizeObserver | null = null;
  private roomTimer: ReturnType<typeof setTimeout> | null = null;
  private scrollHandler: (() => void) | null = null;
  private rescanTimer: ReturnType<typeof setTimeout> | null = null;
  private spyFrame = 0;

  private attach(): void {
    const body = this.host.nativeElement.closest<HTMLElement>('.mgr__body');
    const scroller = body?.querySelector<HTMLElement>('.mgr__detail');
    if (!body || !scroller) {
      return;
    }
    this.scroller = scroller;
    this.watchRoom(body);
    this.rescan();

    // The editor is not static: selecting another profile replaces it wholesale,
    // and a gated field appears or disappears as its sibling changes. A contents
    // list that went stale on either would send the user somewhere that is no
    // longer there, so it follows the DOM instead of being told.
    //
    // `childList` only: a section or a row arriving and leaving is a node
    // change, and watching `characterData` as well woke the observer on every
    // character typed into every field for a set of titles that never move.
    this.observer = new MutationObserver(() => this.scheduleRescan());
    this.observer.observe(scroller, { childList: true, subtree: true });

    this.scrollHandler = () => this.scheduleSpy();
    scroller.addEventListener('scroll', this.scrollHandler, { passive: true });
  }

  /**
   * Keep {@link roomForRail} in step with the space the page actually has.
   *
   * The rule itself is [`hasRoomForRail`]; both of its inputs are read from the
   * live layout rather than assumed, because the list column is draggable and
   * the gap is a token.
   */
  private watchRoom(body: HTMLElement): void {
    const measure = () => {
      const list = body.querySelector<HTMLElement>('.mgr__list');
      const gap = parseFloat(getComputedStyle(body).columnGap) || 0;
      const listWidth = list?.getBoundingClientRect().width ?? 0;
      this.roomForRail.set(hasRoomForRail(body.getBoundingClientRect().width, listWidth, gap));
    };
    measure();

    // Answered after the callback returns, not inside it. The answer adds or
    // removes a grid track, so writing it synchronously resizes the observed
    // subtree from within its own delivery — which the browser cuts short
    // ("ResizeObserver loop completed with undelivered notifications"), dropping
    // the very notification that would have corrected the result. That left the
    // rail showing at widths it had already outgrown.
    //
    // A timeout rather than a frame: `requestAnimationFrame` does not run in a
    // hidden tab, so a window resized while Settings sat in the background
    // stayed wrong until something painted.
    this.roomObserver = new ResizeObserver(() => {
      if (this.roomTimer !== null) {
        return;
      }
      this.roomTimer = setTimeout(() => {
        this.roomTimer = null;
        measure();
      });
    });
    this.roomObserver.observe(body);
  }

  private detach(): void {
    this.observer?.disconnect();
    this.observer = null;
    this.roomObserver?.disconnect();
    this.roomObserver = null;
    if (this.roomTimer !== null) {
      clearTimeout(this.roomTimer);
      this.roomTimer = null;
    }
    if (this.scroller && this.scrollHandler) {
      this.scroller.removeEventListener('scroll', this.scrollHandler);
    }
    this.scrollHandler = null;
    if (this.rescanTimer !== null) {
      clearTimeout(this.rescanTimer);
      this.rescanTimer = null;
    }
    if (this.spyFrame) {
      cancelAnimationFrame(this.spyFrame);
      this.spyFrame = 0;
    }
  }

  /** Coalesce a burst of mutations into one rescan, once the editor settles. */
  private scheduleRescan(): void {
    if (this.rescanTimer !== null) {
      clearTimeout(this.rescanTimer);
    }
    this.rescanTimer = setTimeout(() => {
      this.rescanTimer = null;
      this.rescan();
      this.spy();
    }, RESCAN_QUIET_MS);
  }

  /**
   * One "you are here" update per frame.
   *
   * Kept on its own handle rather than sharing the rescan's: while they shared
   * one, scrolling during a pending rescan cancelled it and the outline stayed
   * stale until the next mutation.
   */
  private scheduleSpy(): void {
    if (this.spyFrame) {
      return;
    }
    this.spyFrame = requestAnimationFrame(() => {
      this.spyFrame = 0;
      this.spy();
    });
  }

  private rescan(): void {
    // Nothing to read while the rail is not on screen, and nothing to draw with
    // it — the work resumes on the first mutation after it comes back.
    if (!this.scroller || !this.visible()) {
      return;
    }
    const sections = scanOutline(this.scroller);
    this.sections.set(sections);
    this.measure(sections);
  }

  private measure(sections: readonly OutlineSection[]): void {
    const scroller = this.scroller;
    if (!scroller) {
      return;
    }
    this.spans = measureOutline(sections, scroller);
    this.measuredHeight = scroller.scrollHeight;
  }

  /** What is on screen, and which section owns the top of it. */
  private spy(): void {
    const scroller = this.scroller;
    const sections = this.sections();
    if (!scroller || sections.length === 0) {
      return;
    }
    // A section expanding, or a lazily-mounted editor arriving, moves every row
    // below it without touching the node list an observer watches. The content
    // height is the cheap tell that the measurements are stale.
    if (scroller.scrollHeight !== this.measuredHeight) {
      this.measure(sections);
    }

    const top = scroller.scrollTop;
    const bottom = top + scroller.clientHeight;
    this.inView.set(idsInView(this.spans, top, bottom));

    let current = sections[0].id;
    for (const section of sections) {
      const span = this.spans.get(section.id);
      if (span && span.top <= top + 1) {
        current = section.id;
      }
    }
    this.currentId.set(current);
  }

  // --- Navigation --------------------------------------------------------

  /** Take the user to a section or a setting, and mark where they landed. */
  protected jump(el: HTMLElement): void {
    el.scrollIntoView({ behavior: 'smooth', block: 'start' });
    // The same mark the "Add & configure" hand-off uses, for the same reason:
    // a smooth scroll ends with the target amongst rows that all look alike.
    el.classList.remove('is-configure-flash');
    void el.offsetWidth;
    el.classList.add('is-configure-flash');
    setTimeout(() => el.classList.remove('is-configure-flash'), FLASH_MS);
  }
}
