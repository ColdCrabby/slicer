import { ChangeDetectionStrategy, Component, effect, inject } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { ActivatedRoute, Router } from '@angular/router';
import { map } from 'rxjs';
import { Viewer } from '../../components/viewer';
import { NotificationService } from '../../services/notifications';
import { Slicer } from '../../services/slicer';
import { SlicerFile, type RequestMeta, type UploadResponse } from '../../services/slicer-file';
import { ViewerControl } from '../../services/viewer-control';

@Component({
  selector: 'nexus-slice-viewer',
  standalone: true,
  imports: [Viewer],
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

  readonly requestUuid = toSignal(
    this.#activatedRoute.params.pipe(map((params) => params['requestUuid'] as string | undefined)),
  );

  /** The user-selected STL (when available) is shown in model mode. */
  readonly modelFile = this.#slicerFile.selectedFile;

  /** Driven by the toolbar toggle; auto-advances to 'gcode' when a slice completes. */
  readonly viewerMode = this.#viewerControl.viewMode;

  #lastFetchedUuid: string | null = null;

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

      const firstFile = meta.ofids[0];
      if (!firstFile) {
        return;
      }

      notifId = this.#notifications.progress(
        'Loading model…',
        `Fetching ${firstFile.original_filename} from server`,
      );

      await this.#slicerFile.fetchFile(
        requestUuid,
        firstFile.file_uuid,
        firstFile.original_filename,
      );

      this.#notifications.completeProgress(notifId, 'Model loaded', firstFile.original_filename);
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
