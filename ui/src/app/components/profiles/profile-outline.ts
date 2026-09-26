import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  afterRenderEffect,
  computed,
  effect,
  inject,
  signal,
  untracked,
  viewChild,
} from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Icon } from '@coldcrabby/ui';
import { Viewport } from '../../services/viewport';
import { KeyboardShortcuts } from '../../services/keyboard-shortcuts/keyboard-shortcuts';
import { NAV_FOLDED_WIDTH, NAV_OPEN_WIDTH, SettingsNav } from '../../services/settings-nav';
import {
  GRAPH_LANES,
  RAIL_WIDTH,
  filterOutline,
  graphLine,
  hasRoomForRail,
  idsInView,
  measureOutline,
  pathData,
  railAnchors,
  railNeedsFold,
  scanOutline,
  sliceLine,
  toRail,
  type OutlineSection,
  type OutlineSpan,
  type RailRow,
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
 * How long after the user touches the rail itself it stops following the
 * editor. Long enough to finish reading what they scrolled to; a follow that
 * yanked the list out from under a finger would make the rail unusable.
 */
const HANDS_OFF_MS = 1500;

/** Room kept between the marked window and the edge of the rail when following. */
const FOLLOW_MARGIN = 24;

/** A section's node on the line. */
interface GraphNode {
  id: string;
  cy: number;
  inWindow: boolean;
  current: boolean;
}

/**
 * The contents rail beside a profile editor: every section of the page, and
 * under each one every setting by name, with a filter box above it.
 *
 * It reads the editor beside it rather than being told what is on it — see
 * `./outline.ts` for why — which is what lets one component serve the printer,
 * filament and print-profile pages without any of them describing themselves
 * twice. Drop it in as a sibling of `.mgr__detail` and it wires itself up.
 *
 * Down its left edge runs one continuous line, drawn the way a git client draws
 * a branch: through a node at each section, swinging in under an open section's
 * settings and back out below them. A thicker stroke on that line marks the
 * part of the editor on screen — mapped pixel for pixel, so it slides as the
 * editor scrolls rather than stepping from row to row. The geometry is in
 * `./outline.ts`; this component only measures and draws.
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
    // The stylesheet indents rows against the same lanes the line is drawn on,
    // and sizes the rail to the width the room test assumes — both from here,
    // so neither can drift from the numbers the geometry uses.
    '[style.--outline-width.px]': 'railWidth',
    '[style.--outline-lane-0.px]': 'lanes[0]',
    '[style.--outline-lane-1.px]': 'lanes[1]',
  },
})
export class ProfileOutline {
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly shortcuts = inject(KeyboardShortcuts);
  private readonly viewport = inject(Viewport);
  private readonly nav = inject(SettingsNav);

  protected readonly railWidth = RAIL_WIDTH;
  protected readonly lanes = GRAPH_LANES;
  /** The SVG only needs to span the lanes, plus a node's radius past the last. */
  protected readonly graphWidth = GRAPH_LANES[GRAPH_LANES.length - 1] + 6;

  /**
   * The rail appears when there is genuinely room for it.
   *
   * The nav, the window and the dragged list width all take from the same
   * budget, and only a measurement of what is left knows about all three — a
   * viewport media query cannot see the other two. See {@link watchRoom}.
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
   * Every row whose target is wholly on screen. Brightens those rows' text — a
   * reading aid; the window drawn on the line is what says how much is showing.
   */
  protected readonly inView = signal<ReadonlySet<string>>(new Set());

  // --- Graph state -------------------------------------------------------

  /** Where each row's target sits in the editor, measured per scan. */
  private readonly spans = signal<ReadonlyMap<string, OutlineSpan>>(new Map());

  /** Where each listed row sits in the rail, measured after every render. */
  private readonly railRows = signal<ReadonlyMap<string, RailRow>>(new Map());

  /** Height of the rail's content, which the SVG must span. */
  protected readonly railHeight = signal(0);

  /** The editor's visible band, in its own content coordinates. */
  private readonly editorWindow = signal({ top: 0, bottom: 0 });

  /** Listed rows in the order they are drawn, as far as they have been measured. */
  private readonly orderedRows = computed<RailRow[]>(() => {
    const measured = this.railRows();
    const ordered: RailRow[] = [];
    for (const section of this.rows()) {
      const row = measured.get(section.id);
      if (row) {
        ordered.push(row);
      }
      if (this.isExpanded(section.id)) {
        for (const entry of section.entries) {
          const entryRow = measured.get(entry.id);
          if (entryRow) {
            ordered.push(entryRow);
          }
        }
      }
    }
    return ordered;
  });

  private readonly line = computed(() => graphLine(this.orderedRows()));

  protected readonly trackPath = computed(() => pathData(this.line()));

  private readonly anchors = computed(() =>
    railAnchors(this.rows(), (id) => this.isExpanded(id), this.spans(), this.railRows()),
  );

  /** The editor's visible band, carried onto the rail. */
  private readonly windowOnRail = computed(() => {
    const anchors = this.anchors();
    const { top, bottom } = this.editorWindow();
    return { top: toRail(anchors, top), bottom: toRail(anchors, bottom) };
  });

  protected readonly windowPath = computed(() => {
    const { top, bottom } = this.windowOnRail();
    return pathData(sliceLine(this.line(), top, bottom));
  });

  protected readonly nodes = computed<GraphNode[]>(() => {
    const { top, bottom } = this.windowOnRail();
    const current = this.currentId();
    return this.orderedRows()
      .filter((row) => row.depth === 0)
      .map((row) => {
        const cy = (row.top + row.bottom) / 2;
        return { id: row.id, cy, inWindow: cy >= top && cy <= bottom, current: row.id === current };
      });
  });

  private scroller: HTMLElement | null = null;

  /** Content height the spans were measured against, to notice a reflow. */
  private measuredHeight = 0;

  /** Until when the rail stops following the editor, after the user touched it. */
  private handsOffUntil = 0;

  private readonly searchInputRef = viewChild<ElementRef<HTMLInputElement>>('searchInput');
  private readonly railBodyRef = viewChild<ElementRef<HTMLElement>>('railBody');
  private readonly railListRef = viewChild<ElementRef<HTMLElement>>('railList');

  constructor() {
    // The same `$mod+f` the slice sidebar's settings search claims. The two are
    // never on screen together — one is the slice page, the other the settings
    // pages — so whichever is mounted answers it. Inside Settings the outline
    // borrows the key from the sidebar's search while it is up, and gives it
    // back when it goes: on a profile editor, "find" means find in this editor.
    const previous = this.shortcuts.settingsSearchRef;
    this.shortcuts.settingsSearchRef = this;
    inject(DestroyRef).onDestroy(() => {
      if (this.shortcuts.settingsSearchRef === this) {
        this.shortcuts.settingsSearchRef = previous;
      }
    });
    afterNextRender(() => this.attach());
    // Unfolding the rail has to read an editor the hidden rail never scanned.
    effect(() => {
      if (this.visible()) {
        this.scheduleRescan();
      }
    });
    // Pressing the fold button changes which of the two widths applies.
    effect(() => {
      this.nav.collapsed();
      untracked(() => this.scheduleRoom?.());
    });
    // The rail's own rows move whenever what it lists changes — a section
    // opens, the filter narrows, the editor is rescanned — so it re-measures
    // after every such render, before the next paint.
    afterRenderEffect({
      read: () => {
        this.rows();
        this.expanded();
        this.query();
        this.visible();
        untracked(() => this.measureRail());
      },
    });
    inject(DestroyRef).onDestroy(() => this.detach());
  }

  // --- Wiring ------------------------------------------------------------

  private observer: MutationObserver | null = null;
  private roomObserver: ResizeObserver | null = null;
  private railObserver: ResizeObserver | null = null;
  private roomTimer: ReturnType<typeof setTimeout> | null = null;
  private scrollHandler: (() => void) | null = null;
  private rescanTimer: ReturnType<typeof setTimeout> | null = null;
  private spyFrame = 0;
  /** Re-run the room test; set once {@link watchRoom} has something to measure. */
  private scheduleRoom: (() => void) | null = null;

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

    // Row heights can change without anything the effect above tracks — the
    // web font arriving, a coarse pointer's taller rows — and the SVG has to
    // follow them.
    const list = this.railListRef()?.nativeElement;
    if (list) {
      this.railObserver = new ResizeObserver(() => this.measureRail());
      this.railObserver.observe(list);
    }
  }

  /**
   * Keep {@link roomForRail} in step with the space the page actually has, and
   * fold the Settings section list when that is what makes the room.
   *
   * The rule itself is [`hasRoomForRail`]; every input is read from the live
   * layout rather than assumed, because the list column is draggable and the
   * gaps are tokens.
   *
   * Inside the Settings shell the widths are worked out from the shell rather
   * than read off the body, because the body is exactly what the section list
   * resizes when it folds: measured mid-animation it would say whatever the
   * animation had reached. The shell's own width does not move, so the body
   * each state *would* give is a subtraction, and the answer is the same
   * before, during and after the fold.
   */
  private watchRoom(body: HTMLElement): void {
    const shell = body.closest<HTMLElement>('.settings');
    const page = body.closest<HTMLElement>('.mgr');
    const list = body.querySelector<HTMLElement>('.mgr__list');

    const measure = () => {
      const gap = parseFloat(getComputedStyle(body).columnGap) || 0;
      const listWidth = list?.getBoundingClientRect().width ?? 0;
      if (this.viewport.isHandheld()) {
        this.nav.requestFold(false);
        this.roomForRail.set(false);
        return;
      }
      if (!shell || !page) {
        this.roomForRail.set(hasRoomForRail(body.getBoundingClientRect().width, listWidth, gap));
        return;
      }
      const style = getComputedStyle(page);
      const padding = (parseFloat(style.paddingLeft) || 0) + (parseFloat(style.paddingRight) || 0);
      const shellWidth = shell.getBoundingClientRect().width;
      const whenOpen = shellWidth - NAV_OPEN_WIDTH - padding;
      const whenFolded = shellWidth - NAV_FOLDED_WIDTH - padding;
      this.nav.requestFold(railNeedsFold(whenOpen, whenFolded, listWidth, gap));
      const folded = untracked(() => this.nav.collapsed());
      this.roomForRail.set(hasRoomForRail(folded ? whenFolded : whenOpen, listWidth, gap));
    };
    const schedule = () => {
      // Answered after the callback returns, not inside it. The answer adds or
      // removes a grid track, so writing it synchronously resizes the observed
      // subtree from within its own delivery — which the browser cuts short
      // ("ResizeObserver loop completed with undelivered notifications"),
      // dropping the very notification that would have corrected the result.
      //
      // A timeout rather than a frame: `requestAnimationFrame` does not run in
      // a hidden tab, so a window resized while Settings sat in the background
      // stayed wrong until something painted.
      if (this.roomTimer !== null) {
        return;
      }
      this.roomTimer = setTimeout(() => {
        this.roomTimer = null;
        measure();
      });
    };
    measure();

    // The body for the window, the list for its resize handle — dragging the
    // list wider changes the budget without changing the body at all.
    this.roomObserver = new ResizeObserver(schedule);
    this.roomObserver.observe(shell ?? body);
    if (list) {
      this.roomObserver.observe(list);
    }
    this.scheduleRoom = schedule;
  }

  private detach(): void {
    this.nav.requestFold(false);
    this.scheduleRoom = null;
    this.observer?.disconnect();
    this.observer = null;
    this.roomObserver?.disconnect();
    this.roomObserver = null;
    this.railObserver?.disconnect();
    this.railObserver = null;
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
      this.follow();
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
    this.spans.set(measureOutline(sections, scroller));
    this.measuredHeight = scroller.scrollHeight;
  }

  /**
   * Where every listed row sits in the rail's own content.
   *
   * Read off the rendered rows by their `data-row` id; a row inside a folded
   * section has no box and is simply not drawn.
   */
  private measureRail(): void {
    const list = this.railListRef()?.nativeElement;
    if (!list || !this.visible()) {
      return;
    }
    const origin = list.getBoundingClientRect().top;
    const rows = new Map<string, RailRow>();
    for (const el of Array.from(list.querySelectorAll<HTMLElement>('[data-row]'))) {
      const rect = el.getBoundingClientRect();
      if (rect.height === 0) {
        continue;
      }
      const id = el.dataset['row']!;
      rows.set(id, {
        id,
        depth: el.dataset['depth'] === '1' ? 1 : 0,
        top: rect.top - origin,
        bottom: rect.bottom - origin,
      });
    }
    this.railRows.set(rows);
    this.railHeight.set(list.scrollHeight);
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
    const spans = this.spans();
    this.editorWindow.set({ top, bottom });
    this.inView.set(idsInView(spans, top, bottom));

    let current = sections[0].id;
    for (const section of sections) {
      const span = spans.get(section.id);
      if (span && span.top <= top + 1) {
        current = section.id;
      }
    }
    this.currentId.set(current);
  }

  /**
   * Keep the marked window inside the rail as the editor scrolls.
   *
   * A long outline scrolls on its own, and a window drawn below the fold of the
   * rail is a map with the "you are here" pin off the edge. Only the editor's
   * scrolling drives it, and never within a moment of the user touching the
   * rail — they are reading it.
   */
  private follow(): void {
    const body = this.railBodyRef()?.nativeElement;
    if (!body || performance.now() < this.handsOffUntil) {
      return;
    }
    const { top, bottom } = this.windowOnRail();
    if (bottom <= top) {
      return;
    }
    const viewTop = body.scrollTop;
    const viewHeight = body.clientHeight;
    let next = viewTop;
    if (bottom - top > viewHeight - FOLLOW_MARGIN * 2 || top < viewTop + FOLLOW_MARGIN) {
      next = top - FOLLOW_MARGIN;
    } else if (bottom > viewTop + viewHeight - FOLLOW_MARGIN) {
      next = bottom - viewHeight + FOLLOW_MARGIN;
    }
    next = Math.max(0, Math.round(next));
    if (next !== Math.round(viewTop)) {
      body.scrollTop = next;
    }
  }

  /** The user is working the rail directly; stop following for a moment. */
  protected handsOn(): void {
    this.handsOffUntil = performance.now() + HANDS_OFF_MS;
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
