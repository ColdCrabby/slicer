import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  computed,
  inject,
  signal,
} from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Icon } from '@coldcrabby/ui';
import { filterOutline, scanOutline, type OutlineSection } from './outline';

/** How long the landing mark on a jumped-to row lasts; matches `configure-flash`. */
const FLASH_MS = 1600;

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
})
export class ProfileOutline {
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  protected readonly query = signal('');

  /** The editor as it currently stands, rescanned whenever it changes. */
  private readonly sections = signal<OutlineSection[]>([]);

  protected readonly visible = computed(() => filterOutline(this.sections(), this.query()));

  /** Section the editor is scrolled to, so the rail can say "you are here". */
  protected readonly currentId = signal<string | null>(null);

  private scroller: HTMLElement | null = null;

  constructor() {
    afterNextRender(() => this.attach());
    inject(DestroyRef).onDestroy(() => this.detach());
  }

  // --- Wiring ------------------------------------------------------------

  private observer: MutationObserver | null = null;
  private scrollHandler: (() => void) | null = null;
  private pending = 0;

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
    this.observer = new MutationObserver(() => this.scheduleRescan());
    this.observer.observe(scroller, { childList: true, subtree: true, characterData: true });

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
    if (this.pending) {
      cancelAnimationFrame(this.pending);
      this.pending = 0;
    }
  }

  /**
   * Coalesce a burst of mutations into one rescan.
   *
   * Typing in a field mutates the editor on every keystroke; rescanning each
   * time would walk a two-hundred-row form for an answer that has not changed.
   */
  private scheduleRescan(): void {
    if (this.pending) {
      return;
    }
    this.pending = requestAnimationFrame(() => {
      this.pending = 0;
      this.rescan();
      this.spy();
    });
  }

  private scheduleSpy(): void {
    if (this.pending) {
      return;
    }
    this.pending = requestAnimationFrame(() => {
      this.pending = 0;
      this.spy();
    });
  }

  private rescan(): void {
    if (!this.scroller) {
      return;
    }
    this.sections.set(scanOutline(this.scroller));
  }

  /** Whichever section has most recently passed under the top of the editor. */
  private spy(): void {
    const scroller = this.scroller;
    const sections = this.sections();
    if (!scroller || sections.length === 0) {
      return;
    }
    const top = scroller.getBoundingClientRect().top + 1;
    let current = sections[0].id;
    for (const section of sections) {
      if (section.el.getBoundingClientRect().top <= top) {
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
