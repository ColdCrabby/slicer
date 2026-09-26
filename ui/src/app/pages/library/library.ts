import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  computed,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { ActivatedRoute, Router } from '@angular/router';
import { map } from 'rxjs';
import { Button, EmptyState, Icon, IconButton, SectionHeader } from '@coldcrabby/ui';
import { LibraryBrowser } from '../../components/library/library-browser';
import { LibraryInspector } from '../../components/library/library-inspector';
import { LibraryOverview } from '../../components/library/library-overview';
import { isTauriMobile } from '../../runtime/domain/runtime-mode.util';
import { ObjectLibrary, type LibraryEntry } from '../../services/library';
import { LibraryActions } from '../../services/library/library-actions';
import { MODEL_FILE_ACCEPT, isSupportedModelFile } from '../../services/model-source';
import { NotificationService } from '../../services/notifications';

/**
 * The library, full page: every model that has reached a plate, as pictures,
 * with the one selected up close beside them.
 *
 * This is the page for looking through the library. When a plate is on
 * screen, the rail opens the library as a flyout beside it instead — that is
 * for putting something on the plate, and leaving the plate would only get in
 * the way.
 *
 * The side panel is never idle: with a model selected it shows that model,
 * and with none it shows the library itself — its size, how it keeps files,
 * and the folders feeding it.
 */
@Component({
  selector: 'nexus-library-page',
  imports: [
    Button,
    EmptyState,
    Icon,
    IconButton,
    LibraryBrowser,
    LibraryInspector,
    LibraryOverview,
    SectionHeader,
  ],
  templateUrl: './library.html',
  styleUrl: './library.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '(keydown.escape)': 'deselect()' },
})
export class LibraryPage {
  protected readonly library = inject(ObjectLibrary);
  readonly #actions = inject(LibraryActions);
  readonly #notifications = inject(NotificationService);
  readonly #router = inject(Router);
  readonly #route = inject(ActivatedRoute);

  protected readonly accept = MODEL_FILE_ACCEPT;
  private readonly fileInput = viewChild.required<ElementRef<HTMLInputElement>>('fileInput');
  private readonly inspector = viewChild(LibraryInspector);

  /** Selection lives in the URL, so the flyout can link straight to a model. */
  protected readonly selectedId = toSignal(
    this.#route.queryParamMap.pipe(map((params) => params.get('model'))),
    { initialValue: null },
  );
  protected readonly selected = computed(
    () => this.library.entries().find((e) => e.id === this.selectedId()) ?? null,
  );
  protected readonly dragActive = signal(false);
  #dragDepth = 0;

  protected readonly description = computed(() => {
    const count = this.library.entries().length;
    return count === 0
      ? 'Every model you put on a plate'
      : `${count} ${count === 1 ? 'model' : 'models'}`;
  });

  /** Where the user can drop models in on this device, when anywhere. */
  protected readonly folderHint = isTauriMobile()
    ? 'Models saved to Files › On My iPad › Cold Crabby › Models appear here too.'
    : null;

  constructor() {
    void this.library.refresh();
  }

  protected select(entry: LibraryEntry | null): void {
    void this.#router.navigate([], {
      relativeTo: this.#route,
      queryParams: { model: entry && entry.id !== this.selectedId() ? entry.id : null },
      replaceUrl: true,
    });
  }

  protected deselect(): void {
    if (this.selectedId()) {
      this.select(null);
    }
  }

  protected activate(entry: LibraryEntry): void {
    void this.#actions.addToPlate(entry);
  }

  /** Rename from a card's menu: select it, then hand over to the name field. */
  protected rename(entry: LibraryEntry): void {
    if (this.selectedId() !== entry.id) {
      this.select(entry);
    }
    setTimeout(() => this.inspector()?.focusName());
  }

  protected pickFiles(): void {
    this.fileInput().nativeElement.click();
  }

  protected onFilesPicked(event: Event): void {
    const input = event.target as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    input.value = '';
    this.#import(files);
  }

  #import(files: readonly File[]): void {
    const models = files.filter((f) => isSupportedModelFile(f.name));
    if (models.length === 0) {
      if (files.length > 0) {
        this.#notifications.error('Nothing to add', 'Use an STL, OBJ or 3MF model.');
      }
      return;
    }
    void this.library.importFiles(models);
  }

  protected async rescan(): Promise<void> {
    await this.library.refresh({ scan: true });
  }

  // ── Drag and drop ─────────────────────────────────────────────────────────

  protected onDragEnter(event: DragEvent): void {
    if (!Array.from(event.dataTransfer?.types ?? []).includes('Files')) {
      return;
    }
    event.preventDefault();
    this.#dragDepth += 1;
    this.dragActive.set(true);
  }

  protected onDragOver(event: DragEvent): void {
    event.preventDefault();
  }

  protected onDragLeave(event: DragEvent): void {
    event.preventDefault();
    this.#dragDepth = Math.max(0, this.#dragDepth - 1);
    if (this.#dragDepth === 0) {
      this.dragActive.set(false);
    }
  }

  protected onDrop(event: DragEvent): void {
    event.preventDefault();
    this.#dragDepth = 0;
    this.dragActive.set(false);
    this.#import(Array.from(event.dataTransfer?.files ?? []));
  }
}
