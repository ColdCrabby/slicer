import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  signal,
} from '@angular/core';
import { RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { resolveRuntimeMode } from '../../runtime/domain/runtime-mode.util';
import { SAVE_DEBOUNCE_MS } from '../../services/profiles/engine-write-through';
import { ProfileSync, type ProfileSyncStatus } from '../../services/profiles/profile-sync';
import { Icon } from '@coldcrabby/ui';

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
  private readonly profileSync = inject(ProfileSync);

  protected readonly sections: SettingsSection[] = [
    { path: 'general', label: 'General', icon: 'control-slider' },
    { path: 'appearance', label: 'Appearance', icon: 'palette' },
    { path: 'printers', label: 'Printers', icon: 'printer' },
    { path: 'filaments', label: 'Filaments', icon: 'droplet' },
    { path: 'profiles', label: 'Print Profiles', icon: 'reports' },
    { path: 'labels', label: 'Labels', icon: 'label' },
    { path: 'shortcuts', label: 'Shortcuts', icon: 'square-cursor' },
    { path: 'changelog', label: "What's New", icon: 'sparks' },
    { path: 'danger-zone', label: 'Danger Zone', icon: 'warning-triangle' },
  ];

  /** Aggregated profile-library sync status; `idle` renders nothing. */
  protected readonly syncStatus = this.profileSync.status;

  /**
   * Whether to show the indicator. Delayed by the save debounce so a quick save
   * (settled within the debounce window) never flashes it; hidden immediately
   * once sync goes idle.
   */
  protected readonly syncVisible = signal(false);

  /**
   * The status the indicator displays. Held at the last active value while
   * fading out so the label doesn't blank mid-animation.
   */
  private readonly shownStatus = signal<ProfileSyncStatus>('idle');

  /** Short, non-alarming label for the shown sync status. */
  protected readonly syncLabel = computed(() => {
    switch (this.shownStatus()) {
      case 'loading':
        return 'Loading…';
      case 'saving':
        return 'Saving…';
      case 'error':
        return "Couldn't save";
      default:
        return '';
    }
  });

  /** True when the shown status is an error, for the danger styling. */
  protected readonly syncIsError = computed(() => this.shownStatus() === 'error');

  constructor() {
    effect((onCleanup) => {
      const status = this.syncStatus();
      if (status === 'idle') {
        this.syncVisible.set(false);
        return;
      }
      this.shownStatus.set(status);
      const timer = setTimeout(() => this.syncVisible.set(true), SAVE_DEBOUNCE_MS);
      onCleanup(() => clearTimeout(timer));
    });
  }

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
