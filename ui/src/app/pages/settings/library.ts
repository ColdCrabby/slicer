import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Button, Icon, IconButton, SectionHeader } from '@coldcrabby/ui';
import { isTauriMobile, resolveRuntimeMode } from '../../runtime/domain/runtime-mode.util';
import { ObjectLibrary, type StorageMode } from '../../services/library';

const MODE_LABELS: Record<StorageMode, string> = {
  copy: 'Copy',
  reference: 'Link',
  both: 'Both',
};

const MODE_NOTES: Record<StorageMode, string> = {
  copy: 'Each model is copied into the library, so it stays even if the original is moved or deleted.',
  reference:
    'Models stay where they are and the library remembers where. Nothing is copied — but a moved or deleted file goes missing.',
  both: 'The library remembers where each model is and keeps a copy to fall back on if the original goes missing.',
};

/**
 * Settings → Library: how the library keeps models, and which folders feed it.
 *
 * Only what this runtime can actually do is offered. A choice with one
 * possible answer is not a choice, so where copying is the only option — the
 * browser, the cloud server, iPad — the page says what happens instead of
 * showing a control that cannot be changed.
 */
@Component({
  selector: 'nexus-settings-library',
  imports: [Button, Icon, IconButton, SectionHeader],
  templateUrl: './library.html',
  styleUrl: './library.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LibrarySettingsPage {
  protected readonly library = inject(ObjectLibrary);
  protected readonly capabilities = this.library.capabilities;
  protected readonly modes = this.capabilities.modes.map((value) => ({
    value,
    label: MODE_LABELS[value],
  }));
  protected readonly mode = computed(() => this.library.settings().mode ?? 'copy');
  protected readonly modeNote = computed(() => MODE_NOTES[this.mode()]);
  protected readonly folders = computed(() => this.library.settings().folders ?? []);
  protected readonly modelsDir = signal<string | null>(null);

  /** Where the models live, when there is only one way to keep them. */
  protected readonly fixedNote = isTauriMobile()
    ? 'Models are copied into the library. It is a folder in the Files app — On My iPad › Cold Crabby › Models — so anything saved there appears in the library too.'
    : resolveRuntimeMode() === 'cloud'
      ? 'Models are kept on the slicer server, so they are there from any browser that opens it.'
      : 'Models are kept in this browser. Clearing its site data clears the library too.';

  constructor() {
    if (!this.library.loaded()) {
      void this.library.refresh({ scan: false });
    }
    if (resolveRuntimeMode() === 'native' && !isTauriMobile()) {
      void import('@tauri-apps/api/core')
        .then(({ invoke }) => invoke<string>('library_models_dir'))
        .then((dir) => this.modelsDir.set(dir))
        .catch(() => undefined);
    }
  }

  protected async setMode(mode: StorageMode): Promise<void> {
    await this.library.saveSettings({ ...this.library.settings(), mode });
  }

  protected async addFolder(): Promise<void> {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked !== 'string' || this.folders().includes(picked)) {
      return;
    }
    await this.library.saveSettings({
      ...this.library.settings(),
      folders: [...this.folders(), picked],
    });
  }

  protected async removeFolder(folder: string): Promise<void> {
    await this.library.saveSettings({
      ...this.library.settings(),
      folders: this.folders().filter((f) => f !== folder),
    });
  }
}
