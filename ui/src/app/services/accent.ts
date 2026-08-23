import { computed, effect, inject, Injectable, signal } from '@angular/core';
import { BrowserStorage } from './browser-storage';

/** Where the UI accent colour is sourced from. */
export type AccentSource = 'brand' | 'system' | 'custom';

const ACCENT_SOURCE_KEY = 'appearance.accentSource';
const ACCENT_COLOR_KEY = 'appearance.accentColor';

/**
 * Owns the single `--accent` CSS variable that every accent shade derives
 * from. The brand default (molten amber) lives in the stylesheet; this service
 * only writes an inline override when the user opts into the OS accent colour.
 *
 * The OS accent itself is fetched from the desktop shell (see
 * {@link refreshSystemAccent}); on the web build there is no system accent and
 * the brand default always applies.
 */
@Injectable({ providedIn: 'root' })
export class AccentService {
  private readonly storage = inject(BrowserStorage);

  /** True when running inside the Tauri desktop shell. */
  readonly isDesktop = this.detectDesktop();

  private readonly storedSource = this.storage.get(ACCENT_SOURCE_KEY, 'local');
  private readonly storedColor = this.storage.get(ACCENT_COLOR_KEY, 'local');

  /** Accent source; defaults to the OS accent on desktop, brand on the web. */
  readonly source = computed<AccentSource>(() => {
    const raw = this.storedSource();
    if (raw === 'brand' || raw === 'system' || raw === 'custom') {
      return raw;
    }
    return this.isDesktop ? 'system' : 'brand';
  });

  /** OS accent as a `#rrggbb` hex, or null when unknown/unavailable. */
  readonly systemAccent = signal<string | null>(null);

  /** User-picked custom accent hex, or null. */
  readonly customAccent = computed<string | null>(() => this.storedColor());

  /** Accent to apply as an override, or null to fall back to the brand token. */
  readonly effectiveAccent = computed<string | null>(() => {
    const candidate =
      this.source() === 'system'
        ? this.systemAccent()
        : this.source() === 'custom'
          ? this.storedColor()
          : null;
    return candidate && this.isUsableAccent(candidate) ? candidate : null;
  });

  constructor() {
    effect(() => {
      const accent = this.effectiveAccent();
      const root = document.documentElement;
      if (accent) {
        root.style.setProperty('--accent', accent);
      } else {
        root.style.removeProperty('--accent');
      }
    });

    if (this.isDesktop) {
      void this.refreshSystemAccent();
    }
  }

  setSource(source: AccentSource): void {
    this.storage.write(ACCENT_SOURCE_KEY, source, 'local');
  }

  /** Pick a specific brand accent; switches the source to `custom`. */
  setCustomAccent(hex: string): void {
    this.storage.write(ACCENT_COLOR_KEY, hex, 'local');
    this.storage.write(ACCENT_SOURCE_KEY, 'custom', 'local');
  }

  setSystemAccent(hex: string | null): void {
    this.systemAccent.set(hex);
  }

  /** Ask the desktop shell for the current OS accent colour. No-op on the web. */
  async refreshSystemAccent(): Promise<void> {
    if (!this.isDesktop) {
      return;
    }
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const hex = await invoke<string | null>('get_system_accent');
      this.systemAccent.set(typeof hex === 'string' ? hex : null);
    } catch {
      // Command unavailable or failed — keep the brand default.
    }
  }

  private detectDesktop(): boolean {
    return (
      typeof globalThis !== 'undefined' &&
      ('__TAURI_INTERNALS__' in globalThis || '__TAURI__' in globalThis)
    );
  }

  /** Reject unparseable or near-white / near-black accents as unreadable. */
  private isUsableAccent(hex: string): boolean {
    const match = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
    if (!match) {
      return false;
    }
    const n = parseInt(match[1], 16);
    const r = (n >> 16) & 0xff;
    const g = (n >> 8) & 0xff;
    const b = n & 0xff;
    const luminance = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
    return luminance > 0.06 && luminance < 0.85;
  }
}
