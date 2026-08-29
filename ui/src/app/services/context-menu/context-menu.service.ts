import { Injectable, inject } from '@angular/core';
import type { OutputRefSubscription } from '@angular/core';
import { isTauriDesktop } from '../../runtime/domain/runtime-mode.util';
import type { FloatingComponentRef, FloatingReference } from '../../shared/floating';
import { FloatingService } from '../../shared/floating';
import { ContextMenu } from './context-menu';
import type { ContextMenuItem } from './context-menu.model';

/**
 * Gap (px) between the pointer and the menu. A mouse cursor is a single point,
 * so it can sit almost flush; a fingertip covers roughly a 40px disc, and a menu
 * opening underneath it is both hidden and easy to mis-tap.
 */
const POINTER_OFFSET = 2;
const TOUCH_OFFSET = 14;

/**
 * Opens a context menu at the pointer, native when possible.
 *
 * The one call site passes a plain {@link ContextMenuItem} list; this service
 * decides how to paint it:
 *
 * - **Native (Tauri *desktop*):** builds a real OS menu via `@tauri-apps/api/menu`
 *   and `popup()`s it at the cursor. Feels native, respects the platform.
 * - **Everything else — browser *and* Tauri mobile:** renders the
 *   {@link ContextMenu} component through `FloatingService`, positioned at the
 *   pointer and dismissed on outside-click or Escape.
 *
 * Mobile deliberately takes the web path: Tauri gates its whole `menu` module
 * behind `#[cfg(desktop)]`, so on iPadOS the native call rejects and the user
 * gets no menu at all. iOS has no OS-level context menu to borrow anyway.
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

    if (isTauriDesktop()) {
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

    // Touch and pen arrive from the long-press recogniser as PointerEvents; a
    // plain right-click does not, and reads as `undefined` here.
    const pointerType = (event as PointerEvent).pointerType;
    const isTouch = pointerType === 'touch' || pointerType === 'pen';

    const ref = this.#floating.openComponent(ContextMenu, {
      reference,
      interactive: true,
      panelClass: 'nexus-floating--fit',
      options: {
        placement: 'right-start',
        offset: isTouch ? TOUCH_OFFSET : POINTER_OFFSET,
        padding: 8,
        size: true,
      },
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
