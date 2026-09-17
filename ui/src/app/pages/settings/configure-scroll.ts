/**
 * Scroll a profile editor to whatever the caller asked for and briefly flash it.
 *
 * Two callers, one behaviour. A wizard's "Add & configure" hands over the
 * section it did not cover; a field notice hands over the single setting it is
 * about — landing on the page and leaving the reader to find one control among
 * sixty is most of the job undone, and it is the whole reason such a notice
 * links anywhere.
 *
 * Deep targets (e.g. the G-code block) sit *below* lazily-mounted Monaco
 * editors that reflow the page after the initial render, so a single
 * `scrollIntoView` fires too early and lands short. We re-scroll a handful of
 * times over ~0.8s to correct for that late reflow, then flash once. No-op if
 * the target never mounts.
 *
 * `target` is a CSS selector rather than an id so a setting can be addressed by
 * the key the schema already knows it by, without every editor minting an id
 * per field.
 */
export function focusConfigureTarget(target: string): void {
  let attempts = 0;
  const settle = () => {
    const el = document.querySelector<HTMLElement>(target);
    if (!el) {
      if (attempts++ < 10) {
        setTimeout(settle, 100);
      }
      return;
    }
    el.scrollIntoView({ block: 'start' });
    if (attempts++ < 8) {
      setTimeout(settle, 100);
      return;
    }
    el.classList.add('is-configure-flash');
    setTimeout(() => el.classList.remove('is-configure-flash'), 1600);
  };
  settle();
}

/**
 * The selector a `focus` query parameter names.
 *
 * `gcode` is the wizard's own hand-off and predates the rest; anything else is
 * a slicing-parameter key, matched on the anchor each editor stamps on its
 * fields.
 */
export function configureTargetSelector(focus: string | null): string {
  if (!focus) {
    return '#configure-target';
  }
  if (focus === 'gcode') {
    return '#gcode-target';
  }
  return `[data-setting="${CSS.escape(focus)}"]`;
}
