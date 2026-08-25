/**
 * Scroll a just-created profile's editor to the first section the "add" wizard
 * did not cover and briefly flash it, so a user who picked "Add & configure"
 * lands directly on the extra settings.
 *
 * Deep targets (e.g. the G-code block) sit *below* lazily-mounted Monaco
 * editors that reflow the page after the initial render, so a single
 * `scrollIntoView` fires too early and lands short. We re-scroll a handful of
 * times over ~0.8s to correct for that late reflow, then flash once. No-op if
 * the anchor never mounts.
 */
export function focusConfigureTarget(anchorId: string): void {
  let attempts = 0;
  const settle = () => {
    const el = document.getElementById(anchorId);
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
