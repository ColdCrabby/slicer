import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  signal,
} from '@angular/core';
import { RouterLink } from '@angular/router';
import { Icon } from '@coldcrabby/ui';
import { SAVE_DEBOUNCE_MS } from '../../services/profiles/engine-write-through';
import { ProfileSync, type ProfileSyncStatus } from '../../services/profiles/profile-sync';
import { storageNote } from './prefs/storage-note';

/**
 * The foot of the Settings sidebar: whether the library is saving, and where it
 * is kept.
 *
 * "Saving…" is the only confirmation an edit to the library was persisted, so
 * it stays even on a phone, where the rest of the sidebar is a chip strip. The
 * storage line is one line that links to the sentence behind it on General →
 * Your library, where the export that answers it lives too.
 */
@Component({
  selector: 'nexus-settings-nav-footer',
  imports: [Icon, RouterLink],
  templateUrl: './settings-nav-footer.html',
  styleUrl: './settings-nav-footer.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SettingsNavFooter {
  private readonly profileSync = inject(ProfileSync);

  /** Where the library lives in this runtime. */
  protected readonly storage = storageNote();

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
      const status = this.profileSync.status();
      if (status === 'idle') {
        this.syncVisible.set(false);
        return;
      }
      this.shownStatus.set(status);
      const timer = setTimeout(() => this.syncVisible.set(true), SAVE_DEBOUNCE_MS);
      onCleanup(() => clearTimeout(timer));
    });
  }
}
