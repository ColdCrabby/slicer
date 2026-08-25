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
}
