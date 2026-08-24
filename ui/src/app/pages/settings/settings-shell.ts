import { ChangeDetectionStrategy, Component } from '@angular/core';
import { RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { resolveRuntimeMode } from '../../runtime/domain/runtime-mode.util';
import { Icon } from '../../shared/icon/icon';

interface SettingsSection {
  path: string;
  label: string;
  icon: string;
}

/**
 * Where the profile library is persisted for the active runtime, used to
 * reassure the user (or warn them) about what survives clearing this browser.
 *
 * - `device` (native) — saved locally, next to the engine.
 * - `server` (cloud) — saved on the slicer server; safe if this browser is
 *   wiped.
 * - `browser` (web/wasm) — kept only in this browser; losable.
 */
type StorageMode = 'device' | 'server' | 'browser';

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
   * Where the profile library is persisted for the active runtime. Drives the
   * sidebar storage notice.
   */
  protected readonly storageMode: StorageMode = ((): StorageMode => {
    switch (resolveRuntimeMode()) {
      case 'native':
        return 'device';
      case 'cloud':
        return 'server';
      default:
        return 'browser';
    }
  })();
}
