import { computed, effect, inject, Injectable } from '@angular/core';
import { BrowserStorage } from './browser-storage';

const THEME_KEY = 'theme';

@Injectable({
  providedIn: 'root',
})
export class AppTheme {
  private readonly storage = inject(BrowserStorage);

  /** Raw string signal backed by localStorage, kept in sync across tabs. */
  private readonly storedTheme = this.storage.get(THEME_KEY, 'local');

  /**
   * `true` when dark mode is active. Derives from stored value with a fallback
   * to the OS colour-scheme preference.
   */
  readonly isDarkMode = computed<boolean>(() => {
    const stored = this.storedTheme();
    if (stored !== null) {
      return stored === 'dark';
    }
    // Fall back to system preference when no explicit choice is stored
    if (typeof window !== 'undefined' && window.matchMedia) {
      return window.matchMedia('(prefers-color-scheme: dark)').matches;
    }
    return false;
  });

  readonly currentTheme = this.isDarkMode;

  /** True when the user has chosen an explicit theme (not "follow system"). */
  readonly hasExplicitPreference = computed<boolean>(() => this.storedTheme() !== null);

  constructor() {
    // Reactively apply the theme class whenever the signal changes,
    // including cross-tab updates driven by BrowserStorage.
    effect(() => {
      this.applyTheme(this.isDarkMode());
    });
  }

  toggleTheme(): void {
    this.storage.write(THEME_KEY, this.isDarkMode() ? 'light' : 'dark', 'local');
  }

  setTheme(isDark: boolean): void {
    this.storage.write(THEME_KEY, isDark ? 'dark' : 'light', 'local');
  }

  /** Clear the explicit choice so the UI follows the OS colour scheme. */
  useSystemTheme(): void {
    this.storage.write(THEME_KEY, null, 'local');
  }

  private applyTheme(isDark: boolean): void {
    if (isDark) {
      document.documentElement.classList.add('dark');
    } else {
      document.documentElement.classList.remove('dark');
    }
  }
}
