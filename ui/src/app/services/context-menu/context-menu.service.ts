import { Injectable, inject } from '@angular/core';
import type { OutputRefSubscription } from '@angular/core';
import { isTauriHost } from '../../runtime/domain/runtime-mode.util';
import type { FloatingComponentRef, FloatingReference } from '../../shared/floating';
import { FloatingService } from '../../shared/floating';
import { ContextMenu } from './context-menu';
import type { ContextMenuItem } from './context-menu.model';

/**
 * Opens a context menu at the pointer, native when possible.
 *
 * The one call site passes a plain {@link ContextMenuItem} list; this service
 * decides how to paint it:
 *
 * - **Native (Tauri desktop):** builds a real OS menu via `@tauri-apps/api/menu`
 *   and `popup()`s it at the cursor. Feels native, respects the platform.
 * - **Web / cloud (browser):** renders the {@link ContextMenu} component through
 *   `FloatingService`, positioned at the cursor and dismissed on outside-click
 *   or Escape.
 *
 * The Tauri module is imported lazily so the browser bundle never pulls it in.
 */
@Injectable({ providedIn: 'root' })
export class ContextMenuService {
  readonly #floating = inject(FloatingService);

  #openRef: FloatingComponentRef<ContextMenu> | null = null;
  #openSub: OutputRefSubscription | null = null;

  /** Show a context menu for `event`'s pointer position. */
  async open(event: MouseEvent, items: readonly ContextMenuItem[]): Promise<void> {
    event.preventDefault();
    event.stopPropagation();

    if (isTauriHost()) {
      await this.#openNative(items);
      return;
    }
    this.#openWeb(event, items);
  }

  /** Dismiss the web fallback menu, if one is open. */
  close(): void {
    this.#openSub?.unsubscribe();
    this.#openSub = null;
    this.#openRef?.close();
    this.#openRef = null;
  }

  async #openNative(items: readonly ContextMenuItem[]): Promise<void> {
    const { Menu } = await import('@tauri-apps/api/menu');
    const menuItems = items.map((item) =>
      item.separator
        ? { item: 'Separator' as const }
        : { text: item.label, enabled: !item.disabled, action: () => item.action?.() },
    );
    const menu = await Menu.new({ items: menuItems });
    await menu.popup();
  }

  #openWeb(event: MouseEvent, items: readonly ContextMenuItem[]): void {
    this.close();

    const x = event.clientX;
    const y = event.clientY;
    const reference: FloatingReference = {
      getBoundingClientRect: () =>
        ({
          x,
          y,
          top: y,
          left: x,
          right: x,
          bottom: y,
          width: 0,
          height: 0,
          toJSON: () => ({}),
        }) as DOMRect,
    };

    const ref = this.#floating.openComponent(ContextMenu, {
      reference,
      interactive: true,
      panelClass: 'nexus-floating--fit',
      options: { placement: 'right-start', offset: 2, padding: 8, size: true },
      onOutsidePointer: () => this.close(),
      onEscape: () => this.close(),
    });

    ref.setInput('items', items);
    this.#openSub = ref.instance.choose.subscribe((item: ContextMenuItem) => {
      this.close();
      item.action?.();
    });
    this.#openRef = ref;
  }
}
