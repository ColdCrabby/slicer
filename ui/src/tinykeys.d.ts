// Ambient module declaration — tinykeys v3 ships types but its package.json
// "exports" field omits the "types" condition, so TypeScript's bundler-mode
// resolver cannot find them automatically. This shim re-declares the subset
// of the public API used by this project.
declare module 'tinykeys' {
  /** A single press step in a keybinding sequence: [modifiers[], key]. */
  type KeyBindingPress = [mods: string[], key: string | RegExp];

  /**
   * Parses a tinykeys keybinding string into an array of press steps.
   * Example: `"$mod+Shift+z"` → `[['Control', 'Shift'], 'z']`
   */
  function parseKeybinding(str: string): KeyBindingPress[];

  /**
   * Returns `true` when a `KeyboardEvent` matches a single `KeyBindingPress`
   * step (i.e. one element from a `parseKeybinding` result).
   */
  function matchKeybindingPress(event: KeyboardEvent, press: KeyBindingPress): boolean;
}
