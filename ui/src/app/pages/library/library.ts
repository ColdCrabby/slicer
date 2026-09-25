import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  computed,
  effect,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { Router, RouterLink } from '@angular/router';
import {
  Button,
  EmptyState,
  Icon,
  IconButton,
  SectionHeader,
  Segmented,
  type SegmentOption,
} from '@coldcrabby/ui';
import { LibraryCard } from '../../components/library/library-card';
import { LibraryPreview } from '../../components/library/library-preview';
import { isTauriMobile } from '../../runtime/domain/runtime-mode.util';
import { ObjectLibrary, type LibraryEntry } from '../../services/library';
import { MODEL_FILE_ACCEPT, isSupportedModelFile } from '../../services/model-source';
import { NotificationService } from '../../services/notifications';
import { Slicer } from '../../services/slicer';
import { SlicerFile } from '../../services/slicer-file';
import { WorkplateObjects } from '../../services/workplate-objects';

type SortOrder = 'recent' | 'added' | 'name';

/** How long the remove button waits for its second press. */
const CONFIRM_MS = 3_000;

/**
 * The library: every model that has reached a plate, as pictures.
 *
 * The page is where a plate gets built from what the user already has — pick a
 * model, see it turn, put it on the open plate or start a new one with it. On
 * an iPad that replaces the system file picker for everything already in the
 * library, which is most of what anyone prints.
 */
@Component({
  selector: 'nexus-library-page',
  imports: [
    Button,
    EmptyState,
    Icon,
    IconButton,
    LibraryCard,
    LibraryPreview,
    RouterLink,
    SectionHeader,
    Segmented,
  ],
  templateUrl: './library.html',
  styleUrl: './library.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LibraryPage {
  protected readonly library = inject(ObjectLibrary);
  readonly #slicer = inject(Slicer);
  readonly #slicerFile = inject(SlicerFile);
  readonly #workplate = inject(WorkplateObjects);
  readonly #router = inject(Router);
  readonly #notifications = inject(NotificationService);

  protected readonly accept = MODEL_FILE_ACCEPT;
  private readonly fileInput = viewChild.required<ElementRef<HTMLInputElement>>('fileInput');

  protected readonly query = signal('');
  protected readonly sort = signal<SortOrder>('recent');
  protected readonly selectedId = signal<string | null>(null);
  protected readonly dragActive = signal(false);
  protected readonly confirmingRemove = signal(false);
  protected readonly busy = signal(false);
  /** The selected model's file, for the live preview. */
  protected readonly previewFile = signal<File | null>(null);
  #dragDepth = 0;
  #confirmTimer: ReturnType<typeof setTimeout> | null = null;

  /** Where the user can drop models in on this device, when anywhere. */
  protected readonly folderHint = isTauriMobile()
    ? 'Models saved to Files › On My iPad › Cold Crabby › Models appear here too.'
    : null;

  /** The open plate, if there is one to add to. */
  protected readonly openPlate = this.#slicerFile.requestUuid;

  protected readonly entries = computed(() => {
    const query = this.query().trim().toLowerCase();
    const filtered = this.library
      .entries()
      .filter((e) => !query || e.name.toLowerCase().includes(query));
    const sort = this.sort();
    return [...filtered].sort((a, b) => {
      switch (sort) {
        case 'name':
          return a.name.localeCompare(b.name, undefined, { numeric: true });
        case 'added':
          return b.added_at.localeCompare(a.added_at);
        default:
          return (b.last_used_at ?? b.added_at).localeCompare(a.last_used_at ?? a.added_at);
      }
    });
  });

  protected readonly selected = computed(
    () => this.library.entries().find((e) => e.id === this.selectedId()) ?? null,
  );

  protected readonly selectedMissing = computed(
    () => !(this.selected()?.locations ?? []).some((l) => !l.missing),
  );

  protected readonly sortOptions: SegmentOption[] = [
    { value: 'recent', label: 'Recent' },
    { value: 'added', label: 'Added' },
    { value: 'name', label: 'Name' },
  ];

  constructor() {
    void this.library.refresh();

    // Load the selected model for the live preview, dropping a stale load.
    effect((onCleanup) => {
      const entry = this.selected();
      const id = entry?.id;
      let current = true;
      onCleanup(() => (current = false));
      this.previewFile.set(null);
      this.confirmingRemove.set(false);
      if (!entry || this.selectedMissing()) {
        return;
      }
      void this.library.open(entry).then((file) => {
        if (current && this.selectedId() === id) {
          this.previewFile.set(file);
        }
      });
    });
  }

  protected setSort(value: string): void {
    this.sort.set(value as SortOrder);
  }

  protected select(entry: LibraryEntry): void {
    this.selectedId.set(this.selectedId() === entry.id ? null : entry.id);
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

  // ── Plate actions ─────────────────────────────────────────────────────────

  /** Start a new plate with this model on it. */
  protected async openAsPlate(entry: LibraryEntry): Promise<void> {
    await this.#withFile(entry, async (file) => {
      const started = await this.#slicer.startWorkplate(file);
      await this.#router.navigate(['/slice', started.requestUuid], {
        state: started.uploadMeta ? { uploadMeta: started.uploadMeta } : undefined,
      });
    });
  }

  /**
   * Add this model to the plate that is open. The plate is not on screen here,
   * so the model is parked the way extra dropped files are and added once the
   * plate's scene is back — adding now would be undone by the plate reloading.
   */
  protected async addToPlate(entry: LibraryEntry): Promise<void> {
    const plate = this.openPlate();
    if (!plate) {
      return this.openAsPlate(entry);
    }
    await this.#withFile(entry, async (file) => {
      this.#workplate.queuePending([file]);
      await this.#router.navigate(['/slice', plate]);
    });
  }

  async #withFile(entry: LibraryEntry, use: (file: File) => Promise<void>): Promise<void> {
    if (this.busy()) {
      return;
    }
    this.busy.set(true);
    try {
      const file = await this.library.open(entry);
      if (!file) {
        this.#notifications.error(
          `${entry.name} could not be opened`,
          'Its file has been moved or deleted.',
        );
        return;
      }
      await this.library.touch(entry);
      await use(file);
    } catch (error) {
      this.#notifications.error(
        `${entry.name} could not be opened`,
        error instanceof Error ? error.message : undefined,
      );
    } finally {
      this.busy.set(false);
    }
  }

  // ── Entry edits ───────────────────────────────────────────────────────────

  protected async rename(entry: LibraryEntry, event: Event): Promise<void> {
    const input = event.target as HTMLInputElement;
    const name = input.value.trim();
    if (!name || name === entry.name) {
      input.value = entry.name;
      return;
    }
    await this.library.rename(entry, name);
  }

  /** Inline two-step confirm: the first press arms, the second removes. */
  protected async remove(entry: LibraryEntry): Promise<void> {
    if (!this.confirmingRemove()) {
      this.confirmingRemove.set(true);
      this.#confirmTimer = setTimeout(() => this.confirmingRemove.set(false), CONFIRM_MS);
      return;
    }
    if (this.#confirmTimer) {
      clearTimeout(this.#confirmTimer);
    }
    this.confirmingRemove.set(false);
    this.selectedId.set(null);
    await this.library.remove(entry);
  }

  protected removeLabel(entry: LibraryEntry): string {
    return entry.locations?.some((l) => l.kind === 'copy')
      ? 'Remove from library'
      : 'Forget this model';
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

  // ── Formatting ────────────────────────────────────────────────────────────

  protected size(bytes: number): string {
    return bytes >= 1024 * 1024
      ? `${(bytes / 1024 / 1024).toFixed(1)} MB`
      : `${Math.max(1, Math.round(bytes / 1024))} KB`;
  }

  protected dimensions(entry: LibraryEntry): string | null {
    const e = entry.shape?.extents_mm;
    return e ? e.map((v) => (Math.round(v * 10) / 10).toString()).join(' × ') + ' mm' : null;
  }

  protected date(iso: string | null | undefined): string {
    return iso ? new Date(iso).toLocaleDateString(undefined, { dateStyle: 'medium' }) : '—';
  }
}
