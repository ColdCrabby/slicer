import { Component, input, output, signal } from '@angular/core';
import { Icon } from '@coldcrabby/ui';
import type { ContextMenuItem } from './context-menu.model';

/**
 * Web fallback rendering of a context menu.
 *
 * Only used when the app is *not* running inside the Tauri desktop shell — the
 * native build pops a real OS menu instead (see {@link ContextMenuService}).
 * The panel is positioned by `FloatingService`; this component only paints the
 * items and reports the chosen one.
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

  /** The item whose submenu is showing, if any. */
  protected readonly openSubmenu = signal<ContextMenuItem | null>(null);

  protected roleFor(item: ContextMenuItem): string {
    if (item.submenu) {
      return 'menuitem';
    }
    return item.checked === undefined ? 'menuitem' : 'menuitemcheckbox';
  }

  /**
   * Opening on hover is what makes it a submenu rather than a second click.
   * Moving onto any other item closes whatever was open, so only one flyout is
   * ever on screen.
   */
  protected onHover(item: ContextMenuItem): void {
    this.openSubmenu.set(item.submenu && !item.disabled ? item : null);
  }

  protected onItem(item: ContextMenuItem): void {
    if (item.disabled || item.separator) {
      return;
    }
    if (item.submenu) {
      // A parent has no action of its own; clicking it pins the flyout open for
      // a pointer that arrived by click rather than by hovering across.
      this.openSubmenu.set(item);
      return;
    }
    this.choose.emit(item);
  }
}
