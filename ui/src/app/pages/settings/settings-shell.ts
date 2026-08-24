import { ChangeDetectionStrategy, Component } from '@angular/core';
import { RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { Icon } from '../../shared/icon/icon';

interface SettingsSection {
  path: string;
  label: string;
  icon: string;
}

/** Settings area frame: a section sub-nav on the left, routed content right. */
@Component({
  selector: 'nexus-settings-shell',
  imports: [RouterLink, RouterLinkActive, RouterOutlet, Icon],
  templateUrl: './settings-shell.html',
  styleUrl: './settings-shell.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SettingsShell {
  protected readonly sections: SettingsSection[] = [
    { path: 'general', label: 'General', icon: 'control-slider' },
    { path: 'appearance', label: 'Appearance', icon: 'palette' },
    { path: 'printers', label: 'Printers', icon: 'printer' },
    { path: 'filaments', label: 'Filaments', icon: 'droplet' },
    { path: 'profiles', label: 'Print Profiles', icon: 'reports' },
    { path: 'labels', label: 'Labels', icon: 'label' },
    { path: 'shortcuts', label: 'Shortcuts', icon: 'square-cursor' },
  ];

  /**
   * True only inside the native desktop shell, where settings live in the
   * app's own persistent storage. Every other build (web/cloud) runs in a
   * browser and keeps settings in that browser's local storage, which is wiped
   * by clearing site data or reinstalling the browser.
   */
  protected readonly isDesktop =
    typeof globalThis !== 'undefined' &&
    ('__TAURI_INTERNALS__' in globalThis || '__TAURI__' in globalThis);
}
