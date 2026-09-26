import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Button, Icon, IconButton, RadioGroup, type RadioOption } from '@coldcrabby/ui';
import { isTauriMobile, resolveRuntimeMode } from '../../runtime/domain/runtime-mode.util';
import { ObjectLibrary, type StorageMode } from '../../services/library';
import { formatBytes } from './library-format';

const MODE_OPTIONS: Record<StorageMode, RadioOption> = {
  copy: {
    value: 'copy',
    label: 'Copy into the library',
    description: 'Stays here even if the original is moved or deleted.',
  },
  reference: {
    value: 'reference',
    label: 'Link to the original',
    description: 'Nothing is copied, but a moved or deleted file goes missing.',
  },
  both: {
    value: 'both',
    label: 'Link, and keep a copy',
    description: 'Opens the original, and falls back on the copy if it goes.',
  },
};

/**
 * The library as a whole — shown beside the grid while no model is selected.
 *
 * This is where the library's own choices live: how models are kept and which
 * folders feed it. They sit next to what they govern rather than in Settings,
 * and only a choice this runtime can actually make is offered — where copying
 * is the only way, it says what happens instead of showing a control with one
 * answer.
 */
@Component({
  selector: 'nexus-library-overview',
  imports: [Button, Icon, IconButton, RadioGroup],
  templateUrl: './library-overview.html',
  styleUrl: './library-overview.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LibraryOverview {
  protected readonly library = inject(ObjectLibrary);
  protected readonly capabilities = this.library.capabilities;

  protected readonly modeOptions = this.capabilities.modes.map((mode) => MODE_OPTIONS[mode]);
  protected readonly mode = computed(() => this.library.settings().mode ?? 'copy');
  protected readonly folders = computed(() => this.library.settings().folders ?? []);
  protected readonly modelsDir = signal<string | null>(null);

  protected readonly count = computed(() => this.library.entries().length);
  protected readonly totalSize = computed(() =>
    formatBytes(this.library.entries().reduce((sum, e) => sum + (e.size ?? 0), 0)),
  );
  protected readonly missing = computed(
    () => this.library.entries().filter((e) => !e.locations?.some((l) => !l.missing)).length,
  );
  /** Files that turned out to be a model already in the library. */
  protected readonly merged = computed(() =>
    this.library.entries().reduce((sum, e) => sum + Math.max(0, (e.hashes?.length ?? 1) - 1), 0),
  );

  /** Where the models live, when there is only one way to keep them. */
  protected readonly fixedNote = isTauriMobile()
    ? 'Models are copied into the library — a folder in the Files app, under On My iPad › Cold Crabby › Models. Anything saved there shows up here too.'
    : resolveRuntimeMode() === 'cloud'
      ? 'Models are kept on the slicer server, so they are here from any browser that opens it.'
      : 'Models are kept in this browser. Clearing its site data clears the library too.';

  constructor() {
    if (this.capabilities.modes.length > 1) {
      void this.library.modelsDir().then((dir) => this.modelsDir.set(dir));
    }
  }

  protected async setMode(mode: string): Promise<void> {
    await this.library.saveSettings({ ...this.library.settings(), mode: mode as StorageMode });
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
