import { computed, inject, Injectable } from '@angular/core';
import { BrowserStorage } from './browser-storage';

/** How a colour swatch behaves when clicked. */
export type ColorPickerMode = 'app' | 'os' | 'auto';

const COLOR_PICKER_KEY = 'appearance.colorPicker';

/**
 * Owns the app-wide default for the colour picker: the in-app popover, the
 * OS-native dialog, or `auto`.
 *
 * The native `<input type="color">` dialog is only the beautiful macOS colour
 * panel in Safari and in WebKit-backed shells (the Tauri desktop app uses
 * WKWebView). Chrome and Firefox — even on macOS — render their own mediocre
 * pickers, and Windows shows an age-old legacy window. So `auto` resolves to
 * the OS picker only when that native panel is actually available and falls
 * back to the polished in-app picker everywhere else.
 */
@Injectable({ providedIn: 'root' })
export class ColorPickerPreference {
  private readonly storage = inject(BrowserStorage);

  /** True when the host OS is macOS. */
  readonly isMac = this.detectMac();

  /**
   * True when the OS-native dialog is the good one — i.e. the macOS colour
   * panel, shown only by WebKit (Safari or the WKWebView desktop shell).
   */
  readonly nativeIsGood = this.isMac && this.detectAppleWebKit();

  private readonly stored = this.storage.get(COLOR_PICKER_KEY, 'local');

  /** The user's chosen mode; defaults to `auto`. */
  readonly mode = computed<ColorPickerMode>(() => {
    const raw = this.stored();
    return raw === 'app' || raw === 'os' || raw === 'auto' ? raw : 'auto';
  });

  /** `auto` collapsed to a concrete choice for the current platform/engine. */
  readonly resolved = computed<'app' | 'os'>(() => {
    const mode = this.mode();
    if (mode === 'auto') {
      return this.nativeIsGood ? 'os' : 'app';
    }
    return mode;
  });

  setMode(mode: ColorPickerMode): void {
    this.storage.write(COLOR_PICKER_KEY, mode, 'local');
  }

  private detectMac(): boolean {
    if (typeof navigator === 'undefined') {
      return false;
    }
    const platform =
      (navigator as { userAgentData?: { platform?: string } }).userAgentData?.platform ??
      navigator.platform ??
      navigator.userAgent;
    return /mac/i.test(platform);
  }

  /**
   * True for Safari or a WKWebView-backed shell (Tauri desktop), false for
   * Chrome/Chromium/Firefox. Apple's WebKit reports vendor "Apple Computer,
   * Inc."; Chrome (vendor "Google Inc.") and Firefox (empty vendor) do not.
   */
  private detectAppleWebKit(): boolean {
    if (typeof globalThis !== 'undefined') {
      if ('__TAURI_INTERNALS__' in globalThis || '__TAURI__' in globalThis) {
        return true;
      }
    }
    if (typeof navigator === 'undefined') {
      return false;
    }
    return navigator.vendor === 'Apple Computer, Inc.';
  }
}
