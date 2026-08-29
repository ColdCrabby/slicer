import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  signal,
} from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { ActivatedRoute, Router } from '@angular/router';
import { map } from 'rxjs';
import { Viewer } from '../../components/viewer';
import { NotificationService } from '../../services/notifications';
import { Slicer } from '../../services/slicer';
import { SlicerFile, type RequestMeta, type UploadResponse } from '../../services/slicer-file';
import { ViewerControl } from '../../services/viewer-control';
import { WorkplateObjects } from '../../services/workplate-objects';
import { Icon } from '../../shared/icon/icon';

@Component({
  selector: 'nexus-slice-viewer',
  standalone: true,
  imports: [Viewer, Icon],
  templateUrl: './slice-viewer.component.html',
  styleUrl: './slice-viewer.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SliceViewer {
  readonly #activatedRoute = inject(ActivatedRoute);
  readonly #router = inject(Router);
  readonly #slicer = inject(Slicer);
  readonly #slicerFile = inject(SlicerFile);
  readonly #notifications = inject(NotificationService);
  readonly #viewerControl = inject(ViewerControl);
  readonly #workplate = inject(WorkplateObjects);

  readonly requestUuid = toSignal(
    this.#activatedRoute.params.pipe(map((params) => params['requestUuid'] as string | undefined)),
  );

  /** The user-selected STL (when available) is shown in model mode. */
  readonly modelFile = this.#slicerFile.selectedFile;

  /**
   * Upload id backing {@link modelFile}, so the object the viewer creates for
   * it can be resolved back to the right bytes at slice time.
   */
  readonly modelSourceId = computed(() => this.#slicerFile.files()[0]?.fileId ?? null);

  /** Driven by the toolbar toggle; auto-advances to 'gcode' when a slice completes. */
  readonly viewerMode = this.#viewerControl.viewMode;

  #lastFetchedUuid: string | null = null;

  /** Highlight the viewport while a file drag is over it. */
  readonly dragActive = signal(false);
  /** Nested dragenter/dragleave pairs, so leaving a child doesn't clear the state. */
  #dragDepth = 0;

  onDragEnter(event: DragEvent): void {
    if (!this.#dragHasFiles(event)) {
      return;
    }
    event.preventDefault();
    this.#dragDepth += 1;
    this.dragActive.set(true);
  }

  onDragOver(event: DragEvent): void {
    if (!this.#dragHasFiles(event)) {
      return;
    }
    // Required for the drop to fire at all.
    event.preventDefault();
  }

  onDragLeave(event: DragEvent): void {
    if (!this.#dragHasFiles(event)) {
      return;
    }
    event.preventDefault();
    this.#dragDepth = Math.max(0, this.#dragDepth - 1);
    if (this.#dragDepth === 0) {
      this.dragActive.set(false);
    }
  }

  /**
   * Drop models onto the open plate to add them.
   *
   * Dropping here *adds* — it never replaces the plate. Starting a fresh plate
   * is what the home screen's drop zone is for.
   */
  onDrop(event: DragEvent): void {
    event.preventDefault();
    this.#dragDepth = 0;
    this.dragActive.set(false);
    const files = Array.from(event.dataTransfer?.files ?? []);
    if (files.length > 0) {
      void this.#addDroppedFiles(files);
    }
  }

  async #addDroppedFiles(files: File[]): Promise<void> {
    const notifId = this.#notifications.progress(
      files.length === 1 ? 'Adding model…' : `Adding ${files.length} models…`,
      files.map((f) => f.name).join(', '),
    );
    try {
      const results = await this.#workplate.addFiles(files);
      const added = results.filter((r) => r.objectId !== undefined);
      const failed = results.filter((r) => r.error);

      if (added.length === 0) {
        this.#notifications.failProgress(
          notifId,
          'Could not add model',
          failed[0]?.error ?? 'Use an STL, OBJ or 3MF model.',
        );
        return;
      }

      this.#notifications.completeProgress(
        notifId,
        added.length === 1 ? 'Model added' : `${added.length} models added`,
        added.map((r) => r.file.name).join(', '),
      );
      for (const failure of failed) {
        this.#notifications.error(`Could not add ${failure.file.name}`, failure.error);
      }
    } catch (error) {
      this.#notifications.failProgress(
        notifId,
        'Could not add model',
        error instanceof Error ? error.message : undefined,
      );
    }
  }

  /** Whether the current drag carries files (ignore text/element drags). */
  #dragHasFiles(event: DragEvent): boolean {
    return Array.from(event.dataTransfer?.types ?? []).includes('Files');
  }

  /**
   * Add any models queued while the plate was being opened.
   *
   * Deliberately driven by the viewer's `loadComplete` rather than the route
   * effect: opening a plate swaps the viewer's `model` input, and that swap
   * tears the scene down. Adding earlier would have the teardown delete the
   * very objects that were just added.
   */
  onViewerLoadComplete(event: { mode: string }): void {
    if (event.mode !== 'model' || this.#workplate.pendingCount() === 0) {
      return;
    }
    void this.#flushQueuedModels();
  }

  async #flushQueuedModels(): Promise<void> {
    const results = await this.#workplate.flushPending();
    const added = results.filter((r) => r.objectId !== undefined);
    if (added.length > 0) {
      this.#notifications.success(
        added.length === 1 ? 'Model added' : `${added.length} models added`,
        added.map((r) => r.file.name).join(', '),
      );
    }
    for (const failure of results.filter((r) => r.error)) {
      this.#notifications.error(`Could not add ${failure.file.name}`, failure.error);
    }
  }

  constructor() {
    // Auto-switch to gcode view as soon as a slice completes.
    effect(() => {
      if (this.#slicer.status() === 'done') {
        this.#viewerControl.viewMode.set('gcode');
      }
    });

    // Always reload the file whenever the route UUID changes — the in-memory
    // file may belong to a different request (e.g. navigating between history
    // entries) or may be absent entirely (reload / deep-link).
    // Guard against double-fire (toSignal init + first emission for same UUID).
    effect(() => {
      const uuid = this.requestUuid();
      if (!uuid || uuid === this.#lastFetchedUuid) {
        return;
      }

      this.#lastFetchedUuid = uuid;
      if (this.#slicerFile.selectedFile() && this.#slicerFile.requestUuid() === uuid) {
        return;
      }

      if (uuid.startsWith('local-')) {
        return;
      }

      void this.#restoreModelFromBackend(uuid);
    });
  }

  async #restoreModelFromBackend(requestUuid: string): Promise<void> {
    let notifId: string | null = null;

    try {
      // If we just navigated here from `slice-new` we already have the upload
      // response in router state — skip the meta fetch entirely.
      const navState = this.#router.getCurrentNavigation()?.extras?.state as
        { uploadMeta?: UploadResponse } | undefined;
      const stateUpload =
        navState?.uploadMeta ?? (history.state?.uploadMeta as UploadResponse | undefined);

      let meta: RequestMeta;
      if (stateUpload && stateUpload.ruuid === requestUuid) {
        // Adopt the upload result immediately, then hydrate it with canonical
        // request metadata (file IDs + original filename) from the backend.
        this.#slicerFile.adopt({
          ruuid: stateUpload.ruuid,
          status: 'upload_complete',
          has_gcode: false,
          ofids: stateUpload.ofids.map((id) => ({ file_uuid: id, original_filename: 'model' })),
        });
        meta = await this.#slicerFile.getRequestMeta(requestUuid);
        this.#slicerFile.adopt(meta);
      } else {
        meta = await this.#slicerFile.getRequestMeta(requestUuid);
        this.#slicerFile.adopt(meta);
      }

      const [firstFile, ...extraFiles] = meta.ofids;
      if (!firstFile) {
        return;
      }

      notifId = this.#notifications.progress(
        'Loading model…',
        `Fetching ${firstFile.original_filename} from server`,
      );

      // The first file seeds the viewer through its `model` input; the rest
      // are added straight to the scene so a plate saved with several objects
      // comes back whole instead of losing everything after the first.
      await this.#slicerFile.fetchFile(
        requestUuid,
        firstFile.file_uuid,
        firstFile.original_filename,
      );

      for (const extra of extraFiles) {
        const file = await this.#slicerFile.downloadFile(
          requestUuid,
          extra.file_uuid,
          extra.original_filename,
        );
        await this.#workplate.addUploadedFile(file, extra.file_uuid);
      }

      const loadedLabel =
        extraFiles.length > 0
          ? `${meta.ofids.length} models restored`
          : firstFile.original_filename;
      this.#notifications.completeProgress(notifId, 'Model loaded', loadedLabel);
    } catch {
      if (notifId) {
        this.#notifications.failProgress(
          notifId,
          'Failed to load model',
          'The model file could not be retrieved from the server.',
        );
      }
    }
  }
}
