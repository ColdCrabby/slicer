import { Component, input, output } from '@angular/core';
import { Icon } from '../../shared/icon/icon';
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

  protected onItem(item: ContextMenuItem): void {
    if (item.disabled || item.separator) {
      return;
    }
    this.choose.emit(item);
  }
}
