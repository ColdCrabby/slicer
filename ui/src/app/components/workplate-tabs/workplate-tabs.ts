import {
  ChangeDetectionStrategy,
  ChangeDetectorRef,
  Component,
  DestroyRef,
  ElementRef,
  TemplateRef,
  afterNextRender,
  afterRenderEffect,
  computed,
  effect,
  inject,
  signal,
  untracked,
  viewChild,
  viewChildren,
} from '@angular/core';
import { Router } from '@angular/router';
import { OpenWorkplateTab, OpenWorkplates } from '../../services/open-workplates';
import { Slicer } from '../../services/slicer';
import { SceneEngine } from '../../services/scene-engine';
import { WorkplateSession } from '../../services/workplate-session';
import { WorkplateNames } from '../../services/workplate-names';
import { ContextMenuService } from '../../services/context-menu/context-menu.service';
import {
  KeyboardShortcuts,
  type WorkplateTabStrip,
} from '../../services/keyboard-shortcuts/keyboard-shortcuts';
import { isTauriHost } from '../../runtime/domain/runtime-mode.util';
import { ContextMenuTrigger } from '../../services/context-menu/context-menu-trigger';
import type { ContextMenuItem } from '../../services/context-menu/context-menu.model';
import {
  FloatingService,
  Icon,
  IconButton,
  TooltipDirective,
  type FloatingRef,
} from '../../ui/shell-primitives';
import { TabSearchEntry, WorkplateTabSearch } from './workplate-tab-search';

/**
 * Open-workplate tab strip shown in the titlebar, replacing the single
 * editable plate-name field. Each tab is an independently renamed, switchable
 * workplate (see {@link OpenWorkplates}); the `+` opens a fresh one, and the
 * chevron beside it lists every open workplate with a search box, for when there
 * are more than the strip can show.
 */
@Component({
  selector: 'nexus-workplate-tabs',
  imports: [Icon, IconButton, TooltipDirective, ContextMenuTrigger, WorkplateTabSearch],
  templateUrl: './workplate-tabs.html',
  styleUrl: './workplate-tabs.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    class: 'nexus-workplate-tabs',
    // The band above the tabs is titlebar too. Tauri only drags from the
    // element that carries the attribute, so the tabs themselves stay clickable.
    'data-tauri-drag-region': '',
    '[hidden]': 'tabs().length === 0 && !isNewPlate()',
  },
})
export class WorkplateTabs implements WorkplateTabStrip {
  private readonly router = inject(Router);
  private readonly slicer = inject(Slicer);
  private readonly sceneEngine = inject(SceneEngine);
  private readonly names = inject(WorkplateNames);
  private readonly openWorkplates = inject(OpenWorkplates);
  private readonly session = inject(WorkplateSession);
  private readonly contextMenu = inject(ContextMenuService);
  private readonly cdr = inject(ChangeDetectorRef);
  private readonly shortcuts = inject(KeyboardShortcuts);
  private readonly floating = inject(FloatingService);

  readonly tabs = this.openWorkplates.tabs;
  readonly activeUuid = this.openWorkplates.activeUuid;
  readonly isNewPlate = this.openWorkplates.isNewPlate;
  /** Whether the tab search list is open. */
  readonly searchOpen = signal(false);
  protected readonly searchShortcut = this.shortcuts.shortcutFor('search-tabs');

  /** Every open tab as the search list shows it — by the name the strip shows. */
  protected readonly searchEntries = computed<TabSearchEntry[]>(() =>
    this.tabs().map((tab) => {
      const name = this.nameFor(tab) || this.placeholderFor(tab);
      // The derived name *is* the filename's stem; repeating it under itself
      // says nothing. It earns the second line once the workplate is renamed.
      const stem = tab.filename?.replace(/\.[^.]+$/, '');
      return { uuid: tab.uuid, name, filename: stem === name ? null : tab.filename };
    }),
  );

  // `read: ElementRef` because the chevron is an `IconButton` host: a bare
  // template reference there resolves to the component, not the element.
  private readonly searchTrigger = viewChild<string, ElementRef<HTMLElement>>('searchTrigger', {
    read: ElementRef,
  });
  private readonly searchPanel = viewChild.required<TemplateRef<unknown>>('searchPanel');
  /** The open search list, if any. */
  private searchRef: FloatingRef | null = null;

  /** UUID of the tab whose name is currently being edited, if any. */
  readonly editingUuid = signal<string | null>(null);

  private readonly editInput = viewChild<ElementRef<HTMLInputElement>>('editInput');
  private readonly tabEls = viewChildren<ElementRef<HTMLElement>>('tabEl');
  private readonly tablist = viewChild.required<ElementRef<HTMLElement>>('tablist');

  /**
   * Width the label occupied when editing began, in px.
   *
   * The editor replaces a `<span>` sized by its text with an `<input>` sized by
   * the browser's default, so without this the tab jumped to a different width
   * the instant you double-clicked it. Held as the input's `min-width`; the
   * input grows past it from there as the name gets longer.
   */
  protected readonly editWidth = signal<number | null>(null);

  /**
   * Live width of the rename box, in px.
   *
   * `field-sizing: content` would do this in CSS, but Safari — and so every
   * iPad — does not implement it, which left the box stuck at its starting
   * width there. Measuring the text against the input's own font is the
   * portable equivalent.
   */
  protected readonly editGrowWidth = signal<number | null>(null);

  constructor() {
    // The `autofocus` attribute is honoured when the parser meets it, so an
    // input swapped in by a control-flow block is simply never focused —
    // Safari in particular ignores it entirely. Focusing here is what makes the
    // box you just opened the box you are typing in; selecting the text means
    // the common case (replace the name outright) needs no extra gesture.
    // Backstop for any path that opens the editor without going through
    // `startEditing` — that method focuses synchronously, and re-focusing an
    // already-focused input is a no-op.
    effect(() => {
      if (this.editInput()) {
        this.focusEditor();
      }
    });

    // Once the strip overflows, the workplate on screen can sit scrolled out of
    // sight — switched to from the search list, a deep link or Home. Bring its
    // tab back into view whenever it changes, once the strip has rendered it,
    // and again whenever the strip itself changes width: the titlebar settles
    // after first render, and a narrower strip can push the tab back out.
    afterRenderEffect(() => {
      this.activeUuid();
      this.tabEls();
      this.revealActiveTab();
    });
    const resize = new ResizeObserver(() => this.revealActiveTab());
    afterNextRender(() => resize.observe(this.tablist().nativeElement));

    // The search list floats through the shared `FloatingService` — the same
    // one the tooltips and context menu use — rather than the CDK overlay, which
    // would put a second positioning engine in the initial bundle for this one
    // popover.
    effect(() => {
      const trigger = this.searchTrigger()?.nativeElement;
      const open = this.searchOpen() && trigger !== undefined;
      untracked(() => {
        if (open && !this.searchRef) {
          this.searchRef = this.openSearchList(trigger);
        } else if (!open) {
          this.closeSearchList();
        }
      });
    });

    // The tab keys reach the strip through the shortcut registry, the same
    // single-slot idiom the settings search uses for `$mod+f`.
    this.shortcuts.tabStripRef = this;
    const destroyRef = inject(DestroyRef);
    destroyRef.onDestroy(() => {
      resize.disconnect();
      this.closeSearchList();
      if (this.shortcuts.tabStripRef === this) {
        this.shortcuts.tabStripRef = null;
      }
    });

    if (isTauriHost()) {
      void this.listenToAppMenu(destroyRef);
    } else {
      this.guardLeavingThePage(destroyRef);
    }
  }

  /**
   * On the web, `Ctrl+W` closes the *browser* tab — no page can intercept it —
   * and muscle memory from the desktop app will press it. While workplates are
   * open, ask before the page goes, so that slip costs a click rather than the
   * whole session. The browser words the question itself; a page only gets to
   * say whether to ask.
   */
  private guardLeavingThePage(destroyRef: DestroyRef): void {
    const onBeforeUnload = (event: BeforeUnloadEvent): void => {
      if (this.tabs().length > 0) {
        event.preventDefault();
      }
    };
    window.addEventListener('beforeunload', onBeforeUnload);
    destroyRef.onDestroy(() => window.removeEventListener('beforeunload', onBeforeUnload));
  }

  /**
   * The Mac app's File menu carries New, Close, Close All and Reopen for
   * workplates — so the keys show up where a Mac user looks for them, and
   * `⌘W` closes a workplate instead of the window. The menu forwards each
   * choice here. Elsewhere nothing emits this event and the listener idles.
   */
  private async listenToAppMenu(destroyRef: DestroyRef): Promise<void> {
    try {
      const { listen } = await import('@tauri-apps/api/event');
      const unlisten = await listen<string>('workplate-menu', ({ payload }) => {
        switch (payload) {
          case 'new':
            void this.addTab();
            break;
          case 'close':
            void this.closeActive();
            break;
          case 'close-all':
            void this.closeAll();
            break;
          case 'reopen':
            this.reopenClosed();
            break;
        }
      });
      destroyRef.onDestroy(unlisten);
    } catch {
      // No event API (a trimmed-down host) — the keydown shortcuts still work.
    }
  }

  /**
   * Close the workplate on screen. On the draft a `+` opened, "close" means
   * give up on it and go back to the last real tab, as closing a browser's new
   * tab does.
   */
  async closeActive(): Promise<void> {
    const uuid = this.activeUuid();
    if (uuid) {
      await this.closeTab(uuid);
      return;
    }
    const last = this.tabs().at(-1);
    if (this.isNewPlate() && last) {
      this.activate(last.uuid);
    }
  }

  reopenClosed(): void {
    this.openWorkplates.reopenLast();
  }

  cycle(step: 1 | -1): void {
    const tabs = this.tabs();
    if (tabs.length === 0) {
      return;
    }
    const index = tabs.findIndex((tab) => tab.uuid === this.activeUuid());
    // From the draft (no tab of its own), forward lands on the first tab and
    // back on the last — it sits past the end of the strip.
    const next = index === -1 ? (step === 1 ? 0 : tabs.length - 1) : index + step;
    this.activate(tabs[(next + tabs.length) % tabs.length].uuid);
  }

  activateIndex(index: number): void {
    const tab = this.tabs().at(index);
    if (tab) {
      this.activate(tab.uuid);
    }
  }

  /** Open the tab search list, or close it if it is already open. */
  toggleSearch(): void {
    if (this.tabs().length === 0) {
      return;
    }
    this.searchOpen.update((open) => !open);
  }

  /** Close the list from the keyboard, handing focus back to the chevron. */
  dismissSearch(): void {
    this.searchOpen.set(false);
    this.searchTrigger()?.nativeElement.focus();
  }

  /** Switch to the workplate picked in the search list. */
  pickFromSearch(uuid: string): void {
    this.searchOpen.set(false);
    this.activate(uuid);
  }

  /**
   * Drop the list below the chevron, right-aligned to it, flipping above when
   * there is no room. A click anywhere but the list closes it — except on the
   * chevron, whose own click toggles it; closing there too would have that
   * click reopen the list.
   */
  private openSearchList(trigger: HTMLElement): FloatingRef {
    return this.floating.openTemplate(
      this.searchPanel(),
      {},
      {
        reference: trigger,
        interactive: true,
        originElement: trigger,
        options: { placement: 'bottom-end', offset: 4, padding: 8 },
        onOutsidePointer: () => this.searchOpen.set(false),
      },
    );
  }

  private closeSearchList(): void {
    this.searchRef?.close();
    this.searchRef = null;
  }

  private revealActiveTab(): void {
    const uuid = this.activeUuid();
    this.tabEls()
      .map((ref) => ref.nativeElement)
      .find((el) => el.getAttribute('data-uuid') === uuid)
      ?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }

  /**
   * Middle-click closes a tab, as it does in every browser. The `mousedown`
   * half stops the middle button's autoscroll from starting on Windows/Linux.
   */
  onTabMouseDown(event: MouseEvent): void {
    if (event.button === 1) {
      event.preventDefault();
    }
  }

  onTabAuxClick(uuid: string, event: MouseEvent): void {
    if (event.button === 1) {
      event.preventDefault();
      void this.closeTab(uuid, event);
    }
  }

  /**
   * A mouse wheel only scrolls vertically, and the strip only scrolls
   * sideways — so without this an overflowing strip could be scrolled by a
   * trackpad but not by a mouse. Horizontal input is left to the browser.
   */
  onStripWheel(event: WheelEvent): void {
    const strip = this.tablist().nativeElement;
    if (
      strip.scrollWidth <= strip.clientWidth ||
      Math.abs(event.deltaY) <= Math.abs(event.deltaX)
    ) {
      return;
    }
    event.preventDefault();
    strip.scrollLeft += event.deltaY;
  }

  /** The stored custom name, if the tab was renamed. */
  nameFor(tab: OpenWorkplateTab): string {
    return this.names.nameFor(tab.uuid) ?? '';
  }

  /** Fallback shown as placeholder when the tab has no custom name yet. */
  placeholderFor(tab: OpenWorkplateTab): string {
    return this.names.displayNameFor(tab.uuid, tab.filename);
  }

  /**
   * A click switches tabs and nothing else.
   *
   * Renaming is a double-click (or the context menu), the way a tab strip
   * behaves everywhere else. Opening an editor on a single click meant every
   * re-click of the tab you were already on dropped a text box in your path,
   * and a click is far too cheap a gesture to start editing on.
   */
  activate(uuid: string): void {
    if (uuid === this.activeUuid()) {
      return;
    }
    void this.router.navigate(['/slice', uuid]);
  }

  startEditing(uuid: string, event?: Event): void {
    event?.preventDefault();
    // Measure before the swap — afterwards the label is gone.
    const label = this.tabEls()
      .map((ref) => ref.nativeElement)
      .find((el) => el.getAttribute('data-uuid') === uuid)
      ?.querySelector<HTMLElement>('.wp-tab-label');
    this.editWidth.set(label ? Math.ceil(label.getBoundingClientRect().width) : null);
    this.editGrowWidth.set(null);
    this.editingUuid.set(uuid);

    // Render the input *now*, still inside the tap that asked for it.
    //
    // iOS and iPadOS only raise the keyboard for a `focus()` that happens in the
    // same task as the user gesture. Left to its own schedule the control-flow
    // block renders a frame later, so by the time the effect below could focus
    // the box the gesture is over — the field takes focus and no keyboard
    // appears, which is exactly what "it doesn't open the keyboard" looked like.
    // Flushing this view synchronously puts the input in the DOM in time.
    this.cdr.detectChanges();
    this.focusEditor();
  }

  /** Focus the rename box and select any existing name. */
  private focusEditor(): void {
    const input = this.editInput()?.nativeElement;
    if (!input) {
      return;
    }
    input.focus();
    // Only select where there is a real name to replace; selecting an empty box
    // whose text is a placeholder reads as a stuck field.
    if (input.value) {
      input.select();
    }
    this.editGrowWidth.set(measureTextWidth(input));
  }

  /**
   * Commit an edit. Clearing the box is a deliberate "use the default again",
   * not a no-op: leaving the old custom name in place made an emptied field
   * look like it had simply failed to save.
   */
  stopEditing(uuid: string, newName: string): void {
    if (this.editingUuid() !== uuid) {
      return;
    }
    // `setName` deletes the entry for a blank value, so this is both "rename"
    // and "go back to the derived name" in one call.
    this.names.setName(uuid, newName);
    this.editingUuid.set(null);
  }

  onInputBlur(uuid: string, event: FocusEvent): void {
    const input = event.target as HTMLInputElement;
    this.stopEditing(uuid, input.value);
  }

  /** Re-measure after each keystroke so the box tracks the text. */
  onInputChanged(input: HTMLInputElement): void {
    this.editGrowWidth.set(measureTextWidth(input));
  }

  onInputKeydown(uuid: string, event: KeyboardEvent, input: HTMLInputElement): void {
    if (event.key === 'Enter') {
      event.preventDefault();
      this.stopEditing(uuid, input.value);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      this.editingUuid.set(null);
    }
  }

  /**
   * Start a new plate.
   *
   * Lands on the dashboard rather than on a bare bed: "new plate" is a question
   * — which model, from where — and the dashboard is where every answer already
   * lives (open a file, drop one, pick up a recent plate, load the demo model).
   * A bare empty bed could only answer one of them.
   *
   * The plate that was on screen is put down, not thrown away: it keeps its tab,
   * and everything it remembers comes back when that tab is clicked.
   */
  addTab(): Promise<void> {
    return this.session.newPlate();
  }

  /** Return to the draft the `+` opened. */
  activateDraft(): void {
    void this.router.navigate(['/']);
  }

  async closeTab(uuid: string, event?: Event): Promise<void> {
    event?.stopPropagation();
    if (uuid === this.activeUuid() && this.tabs().length === 1) {
      // Closing the only open tab leaves nothing to switch to, so the scene is
      // cleared before navigating away rather than bleeding into the dashboard.
      // The plate itself is untouched — it is still in the history list, and
      // reopening it from there brings its objects back.
      await this.slicer.resetWorkplate();
    }
    this.openWorkplates.close(uuid);
  }

  /** Close every tab except `uuid`. */
  closeOthers(uuid: string): void {
    this.openWorkplates.closeAllExcept(uuid);
  }

  /** Close every open tab, clearing the scene since nothing is left on screen. */
  async closeAll(): Promise<void> {
    if (this.tabs().length === 0) {
      return;
    }
    await this.slicer.resetWorkplate();
    this.openWorkplates.closeAllExcept();
  }

  /**
   * Arrow-key movement across the strip, plus Enter/Space to switch and F2 to
   * rename — what `role="tablist"` promises a keyboard user and what a strip of
   * unfocusable `div`s could not deliver. Home/End jump to the ends.
   */
  onTabKeydown(event: KeyboardEvent, uuid: string, index: number): void {
    // Keystrokes in the rename box bubble to the tab, where Space means
    // "activate this tab" — which swallowed every space in a workplate name.
    if (event.target !== event.currentTarget) {
      return;
    }
    const tabs = this.tabs();
    let next: number | null = null;
    switch (event.key) {
      case 'ArrowRight':
        next = (index + 1) % tabs.length;
        break;
      case 'ArrowLeft':
        next = (index - 1 + tabs.length) % tabs.length;
        break;
      case 'Home':
        next = 0;
        break;
      case 'End':
        next = tabs.length - 1;
        break;
      case 'Enter':
      case ' ':
        event.preventDefault();
        this.activate(uuid);
        return;
      case 'F2':
        event.preventDefault();
        this.startEditing(uuid);
        return;
      default:
        return;
    }
    event.preventDefault();
    this.tabEls()[next]?.nativeElement.focus();
  }

  onContextMenu(event: MouseEvent, uuid: string): void {
    const items: ContextMenuItem[] = [
      { label: 'Rename…', icon: 'edit-pencil', action: () => this.startEditing(uuid) },
      // Only the plate on screen is loaded into the scene, so only its tab can
      // export; the item stays visible elsewhere so the action is discoverable.
      {
        label: 'Export as 3MF…',
        icon: 'download',
        disabled: uuid !== this.activeUuid() || this.sceneEngine.objects().length === 0,
        action: () => void this.slicer.exportPlate3mf(),
      },
      { separator: true, label: '' },
      { label: 'Close Tab', icon: 'xmark', action: () => void this.closeTab(uuid) },
      {
        label: 'Close Other Tabs',
        action: () => this.closeOthers(uuid),
        disabled: this.tabs().length <= 1,
      },
      {
        label: 'Close Tabs to the Right',
        action: () => this.openWorkplates.closeToTheRightOf(uuid),
        disabled: this.tabs().at(-1)?.uuid === uuid,
      },
      { label: 'Close All Tabs', action: () => void this.closeAll() },
      { separator: true, label: '' },
      {
        label: 'Search Open Workplates…',
        icon: 'search',
        action: () => this.searchOpen.set(true),
        disabled: this.tabs().length <= 1,
      },
    ];
    void this.contextMenu.open(event, items);
  }
}

/**
 * Width in px of an input's current text, measured in the input's own font.
 *
 * A single reused canvas — creating one per keystroke would be a fresh
 * allocation on every character typed.
 */
let measureCanvas: HTMLCanvasElement | null = null;

function measureTextWidth(input: HTMLInputElement): number | null {
  const text = input.value;
  if (!text) {
    return null;
  }
  measureCanvas ??= document.createElement('canvas');
  const ctx = measureCanvas.getContext('2d');
  if (!ctx) {
    return null;
  }
  // The `font` shorthand serialises to an empty string wherever a longhand it
  // cannot represent is in play — `font-variation-settings`, which this app sets
  // on every text style. Reading the parts is what keeps the measurement in the
  // input's actual face rather than the canvas default of 10px sans-serif.
  const cs = getComputedStyle(input);
  ctx.font = `${cs.fontStyle} ${cs.fontWeight} ${cs.fontSize} / ${cs.lineHeight} ${cs.fontFamily}`;
  // A couple of px of slack so the caret at the end of the text is never
  // sitting on the box's own edge.
  return Math.ceil(ctx.measureText(text).width) + 4;
}
