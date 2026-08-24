/**
 * Scroll a just-created profile's editor to the first section the "add" wizard
 * did not cover and briefly flash it, so a user who picked "Add & configure"
 * lands directly on the extra settings. No-op if the anchor isn't mounted yet.
 */
export function focusConfigureTarget(anchorId: string): void {
  const el = document.getElementById(anchorId);
  if (!el) {
    return;
  }
  el.scrollIntoView({ behavior: 'smooth', block: 'start' });
  el.classList.add('is-configure-flash');
  setTimeout(() => el.classList.remove('is-configure-flash'), 1600);
}
