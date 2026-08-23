import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { AccentService, AccentSource } from '../../services/accent';
import { AppTheme } from '../../services/app-theme';
import { Icon } from '../../shared/icon/icon';
import { SectionHeader } from '../../ui/section-header/section-header';

type ThemeMode = 'light' | 'dark' | 'system';

interface AccentPreset {
  name: string;
  hex: string;
}

@Component({
  selector: 'nexus-settings-appearance',
  imports: [SectionHeader, Icon],
  templateUrl: './appearance.html',
  styleUrl: './appearance.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AppearanceSettings {
  private readonly theme = inject(AppTheme);
  protected readonly accent = inject(AccentService);

  protected readonly themeModes: { value: ThemeMode; label: string }[] = [
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
    { value: 'system', label: 'System' },
  ];

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

  setTheme(mode: ThemeMode): void {
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
