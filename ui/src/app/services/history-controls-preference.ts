import { computed, DestroyRef, inject, Injectable, signal } from '@angular/core';
import { BrowserStorage } from './browser-storage';

/**
 * When the on-canvas undo/redo buttons are shown in the 3D-view toolbar.
 *
 * - `auto` — show them only on devices that are unlikely to have a physical
 *   keyboard (touch-primary tablets and phones), where the ⌘/Ctrl+Z shortcut
 *   is unreachable. Hidden on desktops/laptops, where the shortcut is faster.
 * - `always` / `never` — force the buttons on or off regardless of device.
 */
export type HistoryControlsMode = 'auto' | 'always' | 'never';

const HISTORY_CONTROLS_KEY = 'general.historyControls';

/**
 * Owns the app-wide policy for the toolbar's undo/redo buttons.
 *
 * Undo and redo already have keyboard shortcuts (⌘/Ctrl+Z and friends), so on a
 * desktop the on-canvas buttons are redundant. On a keyboard-less touch device
 * they are the *only* way to step through history, so `auto` reveals them there
 * and hides them elsewhere. The user can override with `always`/`never`.
 *
 * The `auto` detection keys off the primary pointer being coarse
 * (`(pointer: coarse)`), which is what distinguishes a touch tablet/phone from a
 * mouse- or trackpad-driven desktop. It is reactive: plugging in or removing a
 * pointing device flips the media query, and with it the buttons.
 */
@Injectable({ providedIn: 'root' })
export class HistoryControlsPreference {
  private readonly storage = inject(BrowserStorage);
  private readonly stored = this.storage.get(HISTORY_CONTROLS_KEY, 'local');

  /**
   * True when the device is likely keyboard-less — the primary pointer is
   * coarse (touch), as on an iPad or phone. Reactive to device changes.
   */
  private readonly coarsePrimaryPointer = signal(this.detectCoarsePointer());

  /** The user's chosen mode; defaults to `auto`. */
  readonly mode = computed<HistoryControlsMode>(() => {
    const raw = this.stored();
    return raw === 'always' || raw === 'never' || raw === 'auto' ? raw : 'auto';
  });

  /** Whether the toolbar should currently show the undo/redo buttons. */
  readonly visible = computed<boolean>(() => {
    switch (this.mode()) {
      case 'always':
        return true;
      case 'never':
        return false;
      default:
        return this.coarsePrimaryPointer();
    }
  });

  constructor() {
    if (typeof window !== 'undefined' && typeof window.matchMedia === 'function') {
      const query = window.matchMedia('(pointer: coarse)');
      const onChange = (event: MediaQueryListEvent) => this.coarsePrimaryPointer.set(event.matches);
      query.addEventListener('change', onChange);
      inject(DestroyRef).onDestroy(() => query.removeEventListener('change', onChange));
    }
  }

  setMode(mode: HistoryControlsMode): void {
    this.storage.write(HISTORY_CONTROLS_KEY, mode, 'local');
  }

  private detectCoarsePointer(): boolean {
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
      return false;
    }
    return window.matchMedia('(pointer: coarse)').matches;
  }
}
