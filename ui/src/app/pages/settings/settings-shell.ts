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
    { path: 'general', label: 'General', icon: 'settings' },
    { path: 'appearance', label: 'Appearance', icon: 'palette' },
    { path: 'printers', label: 'Printers', icon: 'printer' },
    { path: 'filaments', label: 'Filaments', icon: 'droplet' },
    { path: 'profiles', label: 'Print Profiles', icon: 'reports' },
    { path: 'labels', label: 'Labels', icon: 'label' },
    { path: 'shortcuts', label: 'Shortcuts', icon: 'square-cursor' },
  ];
}
