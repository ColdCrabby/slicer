import type { Type } from '@angular/core';

/**
 * A component rendered inside an item's flyout, instead of nested menu items.
 *
 * Some submenus are a list of commands; some are a *picker*. A picker needs
 * search, its own colours and a way to create the thing being picked, none of
 * which a menu row can carry — so the flyout hosts a real component and the
 * menu just decides where it goes.
 *
 * Outputs are wired by name because the panel is created imperatively; that is
 * also what lets the flyout stay open across several toggles.
 */
export interface ContextMenuPanel {
  component: Type<unknown>;
  inputs?: Record<string, unknown>;
  outputs?: Record<string, (value: never) => void>;
}

/** A single entry in a context menu. */
export interface ContextMenuItem {
  /** Text shown to the user. Ignored when {@link separator} is set. */
  label: string;
  /** Run when the item is chosen. */
  action?: () => void;
  /** Grey the item out and ignore clicks. */
  disabled?: boolean;
  /** Render a divider instead of an item; other fields are ignored. */
  separator?: boolean;
  /**
   * `nexus-icon` name for the web fallback menu. Native OS menus don't render
   * per-item icons, so this is web-only decoration.
   */
  icon?: string;
  /** Destructive-action styling in the web fallback (e.g. Delete). */
  danger?: boolean;
  /**
   * Renders the item as a checkable toggle showing this state.
   *
   * A menu that *assigns* something rather than performing a one-off action —
   * the labels on a profile — has to say what is already assigned, or the user
   * is toggling blind. Drawn natively per platform: a `CheckMenuItem` on the
   * desktop, a checked row in the web menu, and a `✓` in the title on iOS,
   * whose action sheets have no checked state of their own.
   */
  checked?: boolean;
  /**
   * Nested items, opened as a flyout beside this one.
   *
   * A submenu is what a menu does with a list too long or too incidental for
   * the top level. The alternative — an item that swaps the whole menu for a
   * different panel — is not a thing menus do anywhere, and reads as the menu
   * having been closed and something else opened in its place.
   *
   * An item with a submenu performs no action of its own.
   */
  submenu?: ContextMenuItem[];
  /**
   * Rich content for the flyout, used in place of {@link submenu} wherever the
   * menu is drawn by the app rather than by the OS.
   *
   * Both are given for the same item: the native menus on desktop and iOS can
   * only show rows, and rows are the idiomatic thing there anyway.
   */
  submenuPanel?: ContextMenuPanel;
}
