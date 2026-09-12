import {
  Component,
  ElementRef,
  ViewContainerRef,
  effect,
  inject,
  input,
  output,
  signal,
  viewChild,
} from '@angular/core';
import type { ComponentRef, OutputRef } from '@angular/core';
import { Icon } from '@coldcrabby/ui';
import type { ContextMenuItem } from './context-menu.model';

/**
 * Web fallback rendering of a context menu.
 *
 * Only used when the app is *not* running inside the Tauri desktop shell — the
 * native build pops a real OS menu instead (see {@link ContextMenuService}).
 * The panel is positioned by `FloatingService`; this component paints the items,
 * hosts any flyout, and reports the chosen one.
 */
@Component({
  selector: 'nexus-context-menu',
  standalone: true,
  imports: [Icon],
  templateUrl: './context-menu.html',
  styleUrl: './context-menu.scss',
})
export class ContextMenu {
  readonly items = input<readonly ContextMenuItem[]>([]);
  readonly choose = output<ContextMenuItem>();

  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly panelHost = viewChild('panelHost', { read: ViewContainerRef });

  /** The item whose flyout is showing, if any. */
  protected readonly openItem = signal<ContextMenuItem | null>(null);
  /** Whether the flyout opens to the left, because there is no room right. */
  protected readonly flipped = signal(false);

  private panelRef: ComponentRef<unknown> | null = null;

  constructor() {
    // The flyout's content is created imperatively rather than through
    // `ngComponentOutlet`, because a picker has outputs to wire and that
    // directive binds inputs only.
    effect(() => {
      const item = this.openItem();
      const container = this.panelHost();
      this.disposePanel();
      if (!item?.submenuPanel || !container) {
        return;
      }
      const spec = item.submenuPanel;
      const ref = container.createComponent(spec.component);
      // An input may be given as a getter, for a value that has to be re-read
      // after each edit — the assigned labels, which the panel itself changes.
      const applyInputs = (): void => {
        for (const [name, value] of Object.entries(spec.inputs ?? {})) {
          ref.setInput(name, typeof value === 'function' ? (value as () => unknown)() : value);
        }
      };
      applyInputs();
      for (const [name, handler] of Object.entries(spec.outputs ?? {})) {
        const emitter = (ref.instance as Record<string, unknown>)[name] as
          OutputRef<never> | undefined;
        emitter?.subscribe((value) => {
          handler(value);
          // Re-read after every change, so ticks follow the edit — and leave
          // the flyout open, because assigning three labels should be three
          // clicks rather than three trips through the menu.
          applyInputs();
        });
      }
      this.panelRef = ref;
    });
  }

  protected hasFlyout(item: ContextMenuItem): boolean {
    return Boolean(item.submenuPanel ?? item.submenu?.length);
  }

  protected roleFor(item: ContextMenuItem): string {
    if (this.hasFlyout(item)) {
      return 'menuitem';
    }
    return item.checked === undefined ? 'menuitem' : 'menuitemcheckbox';
  }

  /**
   * Opening on hover is what makes it a submenu rather than a second click.
   * Moving onto any other row closes whatever was open, so only one flyout is
   * ever on screen.
   */
  protected onHover(item: ContextMenuItem): void {
    if (item.disabled || !this.hasFlyout(item)) {
      this.openItem.set(null);
      return;
    }
    if (this.openItem() === item) {
      return;
    }
    this.flipped.set(this.wouldOverflowRight());
    this.openItem.set(item);
  }

  protected onItem(item: ContextMenuItem): void {
    if (item.disabled || item.separator) {
      return;
    }
    if (this.hasFlyout(item)) {
      // A parent has no action of its own; clicking it pins the flyout open for
      // a pointer that arrived by click rather than by hovering across.
      this.onHover(item);
      return;
    }
    this.choose.emit(item);
  }

  /** Whether a flyout at the panel's right edge would leave the window. */
  private wouldOverflowRight(): boolean {
    const rect = this.host.nativeElement.getBoundingClientRect();
    // The widest thing a flyout holds is the picker panel; anything narrower
    // simply has more room than this reserves.
    return rect.right + 260 > window.innerWidth;
  }

  private disposePanel(): void {
    this.panelHost()?.clear();
    this.panelRef?.destroy();
    this.panelRef = null;
  }
}
