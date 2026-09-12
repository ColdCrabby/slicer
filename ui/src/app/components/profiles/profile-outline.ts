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
} from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Icon } from '@coldcrabby/ui';
import { SettingsNav } from '../../services/settings-nav';
import {
  filterOutline,
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
  private readonly settingsNav = inject(SettingsNav);

  /**
   * The rail appears only once the Settings section list has been folded to
   * icons.
   *
   * Settings is already sections + list + editor before the outline asks for
   * anything, and a fourth column at once is what made the page feel crowded.
   * Tying the two together makes it a trade the user makes deliberately —
   * fold the sections, gain the contents — rather than a column that turns up
   * uninvited.
   */
  protected readonly visible = this.settingsNav.collapsed;

  protected readonly query = signal('');

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

  constructor() {
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
  private scrollHandler: (() => void) | null = null;
  private rescanTimer: ReturnType<typeof setTimeout> | null = null;
  private spyFrame = 0;

  private attach(): void {
    const scroller = this.host.nativeElement
      .closest('.mgr__body')
      ?.querySelector<HTMLElement>('.mgr__detail');
    if (!scroller) {
      return;
    }
    this.scroller = scroller;
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

  private detach(): void {
    this.observer?.disconnect();
    this.observer = null;
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
