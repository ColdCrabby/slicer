import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { AccentService, type AccentSource } from '../../services/accent';
import { AppTheme } from '../../services/app-theme';
import {
  ColorPickerPreference,
  type ColorPickerMode,
} from '../../services/color-picker-preference';
import { SectionHeader, Segmented, TooltipDirective, type SegmentOption } from '@coldcrabby/ui';
import { PrefRow } from './prefs/pref-row';
import { landOnFragment } from './prefs/land-on-fragment';

type ThemeMode = 'light' | 'dark' | 'system';

interface AccentPreset {
  name: string;
  hex: string;
}

@Component({
  selector: 'nexus-settings-appearance',
  imports: [SectionHeader, Segmented, TooltipDirective, PrefRow],
  templateUrl: './appearance.html',
  styleUrl: './appearance.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AppearanceSettings {
  private readonly theme = inject(AppTheme);
  protected readonly accent = inject(AccentService);
  protected readonly colorPicker = inject(ColorPickerPreference);

  protected readonly themeModes: SegmentOption[] = [
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
    { value: 'system', label: 'System' },
  ];

  protected readonly colorPickerModes: SegmentOption[] = [
    { value: 'app', label: 'Prefer app' },
    { value: 'os', label: 'Prefer OS' },
    { value: 'auto', label: 'Auto' },
  ];

  protected readonly colorPickerHint = computed(() => {
    switch (this.colorPicker.mode()) {
      case 'app':
        return 'Always use the polished in-app picker.';
      case 'os':
        return 'Always open the native system colour dialog.';
      default:
        return this.colorPicker.nativeIsGood
          ? 'Native macOS colour panel here, in-app picker elsewhere.'
          : 'In-app picker here; the native panel only shines in Safari or the desktop app.';
    }
  });

  protected readonly presets: AccentPreset[] = [
    { name: 'Molten Amber', hex: '#e0730f' },
    { name: 'Teal', hex: '#0d8f86' },
    { name: 'Indigo', hex: '#5b62e0' },
    { name: 'Violet', hex: '#7c5cff' },
    { name: 'Rose', hex: '#e0568b' },
    { name: 'Forest', hex: '#3f9d5a' },
  ];

  protected readonly themeMode = computed<ThemeMode>(() =>
    !this.theme.hasExplicitPreference() ? 'system' : this.theme.isDarkMode() ? 'dark' : 'light',
  );

  protected readonly currentPreset = computed(() =>
    this.accent.source() === 'custom' ? this.accent.customAccent() : null,
  );

  /** The accent in use, by name — the swatches alone cannot say "Brand". */
  protected readonly accentHint = computed(() => {
    switch (this.accent.source()) {
      case 'brand':
        return 'Molten amber, the brand default.';
      case 'system':
        return "Following your system's accent colour.";
      default: {
        const preset = this.presets.find((p) => p.hex === this.accent.customAccent());
        return preset ? `${preset.name}, everywhere the app highlights something.` : '';
      }
    }
  });

  constructor() {
    landOnFragment();
  }

  setColorPicker(mode: string): void {
    this.colorPicker.setMode(mode as ColorPickerMode);
  }

  setTheme(value: string): void {
    const mode = value as ThemeMode;
    if (mode === 'system') {
      this.theme.useSystemTheme();
    } else {
      this.theme.setTheme(mode === 'dark');
    }
  }

  setAccentSource(source: AccentSource): void {
    this.accent.setSource(source);
  }

  setPreset(hex: string): void {
    this.accent.setCustomAccent(hex);
  }
}
