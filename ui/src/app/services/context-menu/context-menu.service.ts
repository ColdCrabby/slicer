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
  #detachScrollDismiss: (() => void) | null = null;
  #openNativeMenu: unknown = null;

  /** Show a context menu for `event`'s pointer position. */
  async open(event: MouseEvent, items: readonly ContextMenuItem[]): Promise<void> {
    event.preventDefault();
    event.stopPropagation();

    // A web menu may still be up when the platform path changes under us; the
    // native paths have no equivalent of `#openWeb`'s own `close()` call.
    this.close();

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
    this.#detachScrollDismiss?.();
    this.#detachScrollDismiss = null;
    this.#openSub?.unsubscribe();
    this.#openSub = null;
    this.#openRef?.close();
    this.#openRef = null;
  }

  async #openNative(items: readonly ContextMenuItem[]): Promise<void> {
    const { Menu } = await import('@tauri-apps/api/menu');
    // The union Tauri accepts here is wide and structural; each branch of
    // `toNativeItem` builds one of its shapes, which TypeScript cannot see
    // through a `Record` return.
    const menu = await Menu.new({
      items: items.map(toNativeItem) as unknown as NonNullable<
        Parameters<typeof Menu.new>[0]
      >['items'],
    });
    // Held on the instance for as long as the menu is up. The item callbacks
    // live on the JS side of the Tauri bridge, so letting the only reference go
    // out of scope the moment `popup()` resolves leaves them eligible for
    // collection while the user is still reading the menu.
    this.#openNativeMenu = menu;
    try {
      await menu.popup();
    } finally {
      this.#openNativeMenu = null;
    }
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
    // A UIAlertController has neither submenus nor checked rows, so a nested
    // menu is flattened into the sheet under its parent's name and the tick
    // moves into the title — the only place an action sheet can carry either.
    const flat = flattenForSheet(items);
    const chosen = await invoke<number | null>('show_context_menu', {
      items: flat.map((item) => ({
        label: item.checked ? `\u2713 ${item.label}` : item.label,
        disabled: item.disabled ?? false,
        separator: item.separator ?? false,
        danger: item.danger ?? false,
      })),
      x: event.clientX,
      y: event.clientY,
    });

    if (chosen !== null && chosen !== undefined) {
      flat[chosen]?.action?.();
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

    // The menu is pinned to the viewport point the press happened at, so
    // scrolling the list underneath leaves it pointing at a different row than
    // the one it was opened for. Native menus dismiss on scroll; so does this.
    const onScroll = () => this.close();
    window.addEventListener('scroll', onScroll, { capture: true, passive: true });
    this.#detachScrollDismiss = () =>
      window.removeEventListener('scroll', onScroll, { capture: true });

    ref.setInput('items', items);
    this.#openSub = ref.instance.choose.subscribe((item: ContextMenuItem) => {
      this.close();
      item.action?.();
    });
    this.#openRef = ref;
  }
}

/**
 * One item for `@tauri-apps/api/menu`.
 *
 * `checked` present (even `false`) makes Tauri build a CheckMenuItem, so an
 * unticked box still reads as one thing in a list of toggles; `items` makes it
 * a real OS submenu.
 */
function toNativeItem(item: ContextMenuItem): Record<string, unknown> {
  if (item.separator) {
    return { item: 'Separator' as const };
  }
  if (item.submenu) {
    return {
      text: item.label,
      enabled: !item.disabled,
      items: item.submenu.map(toNativeItem),
    };
  }
  const base: Record<string, unknown> = {
    text: item.label,
    enabled: !item.disabled,
    action: () => item.action?.(),
  };
  return item.checked === undefined ? base : { ...base, checked: item.checked };
}

/**
 * Flatten a menu for an iOS action sheet, which has no nesting.
 *
 * A submenu's children are inlined after a separator, each prefixed with its
 * parent — "Labels: PLA" — so the sheet still says what a row belongs to. The
 * parent row itself is dropped: it has no action, and a row that does nothing
 * when tapped is worse than no row.
 */
function flattenForSheet(items: readonly ContextMenuItem[]): ContextMenuItem[] {
  const out: ContextMenuItem[] = [];
  for (const item of items) {
    if (!item.submenu) {
      out.push(item);
      continue;
    }
    if (item.submenu.length === 0) {
      continue;
    }
    out.push({ separator: true, label: '' });
    for (const child of item.submenu) {
      out.push({ ...child, label: `${item.label}: ${child.label}` });
    }
  }
  return out;
}
