import { Injectable, inject } from '@angular/core';
import type { OutputRefSubscription } from '@angular/core';
import { isTauriDesktop, isTauriHost } from '../../runtime/domain/runtime-mode.util';
import { FloatingService, type FloatingComponentRef, type FloatingReference } from '@coldcrabby/ui';
import { ContextMenu } from './context-menu';
import type { ContextMenuItem } from './context-menu.model';

/**
 * Gap (px) between the pointer and the menu. A mouse cursor is a single point,
 * so the menu can sit almost flush; a fingertip covers roughly a 40px disc, and
 * a menu opening underneath it is both hidden and easy to mis-tap.
 */
const POINTER_OFFSET = 2;
const TOUCH_OFFSET = 14;

/**
 * Opens a context menu at the pointer, natively wherever the OS offers one.
 *
 * The call sites pass a plain {@link ContextMenuItem} list; this service decides
 * how to paint it:
 *
 * - **Tauri desktop:** a real OS menu via `@tauri-apps/api/menu`, popped up at
 *   the cursor.
 * - **Tauri iOS/iPadOS:** a real UIKit action sheet via the `show_context_menu`
 *   command, which UIKit renders as a popover anchored at the touch point. iOS
 *   has no equivalent of `@tauri-apps/api/menu` (Tauri gates that module behind
 *   `#[cfg(desktop)]`), so the native menu is built in Rust instead — see
 *   `ui-desktop/src-tauri/src/context_menu.rs`.
 * - **Browser only:** the {@link ContextMenu} component through
 *   `FloatingService`. A web page has no OS menu to borrow, so this is the one
 *   context where an HTML menu is the *only* option — not a preference.
 *
 * Tauri modules are imported lazily so the browser bundle never pulls them in.
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
    if (isTauriHost()) {
      await this.#openNativeMobile(event, items);
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

  /**
   * iOS/iPadOS: hand the items to UIKit and run whatever comes back.
   *
   * The command resolves with the chosen item's index once the user taps, or
   * `null` if they dismissed the sheet. Indices are used rather than ids
   * because `action` is a closure that cannot cross the IPC boundary — the Rust
   * side only ever sees labels and flags.
   */
  async #openNativeMobile(event: MouseEvent, items: readonly ContextMenuItem[]): Promise<void> {
    const { invoke } = await import('@tauri-apps/api/core');
    const chosen = await invoke<number | null>('show_context_menu', {
      items: items.map((item) => ({
        label: item.label,
        disabled: item.disabled ?? false,
        separator: item.separator ?? false,
        danger: item.danger ?? false,
      })),
      x: event.clientX,
      y: event.clientY,
    });

    if (chosen !== null && chosen !== undefined) {
      items[chosen]?.action?.();
    }
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

    // Browser-only path, but not necessarily a mouse: a tablet or touch laptop
    // reaches this through the long-press recogniser, and a menu that opens
    // under the fingertip is hidden by the finger itself.
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
