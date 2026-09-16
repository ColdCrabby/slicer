import { HttpClient, HttpEventType } from '@angular/common/http';
import { Injectable, computed, inject, signal } from '@angular/core';
import { environment } from '../../environments/environment';
import { CLIENT_ID_HEADER, clientId } from './client-id';

/**
 * Metadata for a workplate, returned by `GET /api/request/:request_uuid`.
 *
 * `ofids` is the list of files (by `file_uuid`) that were placed in this
 * workplate. The slicer references each file by its own UUID — distinct
 * from the workplate `request_uuid`/`ruuid`.
 */
export interface RequestMeta {
  ruuid: string;
  status: string;
  has_gcode: boolean;
  ofids: { file_uuid: string; original_filename: string }[];
}

/**
 * Response from `POST /api/upload` — the workplate UUID plus the list of
 * file UUIDs that were created. Today there is exactly one file per upload,
 * but the protocol is multi-file ready.
 */
export interface UploadResponse {
  ruuid: string;
  ofids: string[];
}

/**
 * One file that belongs to the active workplate.
 *
 * A plate can hold several models, so uploads accumulate rather than replace.
 */
export interface WorkplateFile {
  fileId: string;
  filename: string;
}

@Injectable({ providedIn: 'root' })
export class SlicerFile {
  readonly #http = inject(HttpClient);

  readonly selectedFile = signal<File | null>(null);
  /** Source model filename for the active workplate (used for title/download fallbacks). */
  readonly sourceFilename = signal<string | null>(null);
  /** Workplate UUID — the `ruuid` from the upload response. */
  readonly requestUuid = signal<string | null>(null);
  /** Every file placed on {@link requestUuid}, in the order it was added. */
  readonly files = signal<readonly WorkplateFile[]>([]);
  /** File UUIDs (`ofids`) that belong to {@link requestUuid}. */
  readonly fileIds = computed(() => this.files().map((f) => f.fileId));
  readonly uploadProgress = signal<number>(0);
  readonly uploadError = signal<string | null>(null);
  readonly isUploading = computed(() => this.uploadProgress() > 0 && this.uploadProgress() < 100);
  readonly isPending = computed(
    () =>
      this.selectedFile() !== null && this.uploadProgress() === 0 && this.uploadError() === null,
  );

  selectFile(file: File): void {
    this.selectedFile.set(file);
    this.sourceFilename.set(file.name);
    this.requestUuid.set(null);
    this.files.set([]);
    this.uploadProgress.set(0);
    this.uploadError.set(null);
  }

  /**
   * Upload `file` and return its workplate + file UUIDs.
   *
   * When a workplate is already open the upload is attached to it, so the
   * plate accumulates models instead of each one starting a fresh plate.
   * Pass `attachToWorkplate: false` to force a new plate.
   */
  upload(file?: File, options: { attachToWorkplate?: boolean } = {}): Promise<UploadResponse> {
    const target = file ?? this.selectedFile();
    if (!target) {
      throw new Error('No file selected');
    }

    this.uploadProgress.set(0);
    this.uploadError.set(null);

    const formData = new FormData();
    // The server streams multipart fields in order and needs the workplate id
    // before it starts writing the file, so append `ruuid` first.
    const existing = options.attachToWorkplate === false ? null : this.requestUuid();
    if (existing) {
      formData.append('ruuid', existing);
    }
    formData.append('file', target);

    return new Promise((resolve, reject) => {
      this.#http
        .post<UploadResponse>(`${environment.apiUrl}/upload`, formData, {
          // A model landing on a plate is a change to it; naming the author
          // keeps the engine from announcing it back to this view.
          headers: { [CLIENT_ID_HEADER]: clientId() },
          reportProgress: true,
          observe: 'events',
        })
        .subscribe({
          next: (event) => {
            if (event.type === HttpEventType.UploadProgress) {
              const progress = event.total ? Math.round((event.loaded / event.total) * 100) : 0;
              this.uploadProgress.set(progress);
            } else if (event.type === HttpEventType.Response) {
              const body = event.body;
              if (!body || !body.ruuid || !body.ofids) {
                const message = 'Invalid response from server';
                this.uploadError.set(message);
                reject(new Error(message));
                return;
              }
              this.requestUuid.set(body.ruuid);
              this.addFiles(body.ofids.map((fileId) => ({ fileId, filename: target.name })));
              this.uploadProgress.set(100);
              resolve(body);
            }
          },
          error: (error: unknown) => {
            let message = 'Upload failed';
            if (error instanceof Error) {
              message = error.message;
            } else if (typeof error === 'object' && error !== null && 'error' in error) {
              const err = error as { error?: { message?: string } };
              message = err.error?.message || message;
            }
            this.uploadError.set(message);
            this.uploadProgress.set(0);
            console.error('[SlicerFile] Upload error:', message);
            reject(new Error(message));
          },
        });
    });
  }

  /** Append files to the active workplate, ignoring ones already present. */
  addFiles(entries: readonly WorkplateFile[]): void {
    if (entries.length === 0) {
      return;
    }
    const known = new Set(this.files().map((f) => f.fileId));
    const fresh = entries.filter((e) => e.fileId && !known.has(e.fileId));
    if (fresh.length === 0) {
      return;
    }
    this.files.update((current) => [...current, ...fresh]);
    if (!this.sourceFilename()) {
      this.sourceFilename.set(fresh[0].filename);
    }
  }

  /** Drop a file from the active workplate (its object was removed). */
  removeFile(fileId: string): void {
    this.files.update((current) => current.filter((f) => f.fileId !== fileId));
  }

  reset(): void {
    this.selectedFile.set(null);
    this.sourceFilename.set(null);
    this.requestUuid.set(null);
    this.files.set([]);
    this.uploadProgress.set(0);
    this.uploadError.set(null);
  }

  /** Fetch workplate metadata for a given `ruuid`. */
  getRequestMeta(requestUuid: string): Promise<RequestMeta> {
    return this.#http
      .get<RequestMeta>(`${environment.apiUrl}/request/${requestUuid}`)
      .toPromise()
      .then((meta) => {
        if (!meta) {
          throw new Error('No response from server');
        }
        return meta;
      })
      .catch((error) => {
        const message =
          error instanceof Error ? error.message : 'Failed to fetch workplate metadata';
        console.error('[SlicerFile] getRequestMeta error:', message);
        throw new Error(message);
      });
  }

  /**
   * Adopt a plate whose scene has already been rebuilt.
   *
   * The uuid is set **last**, on purpose: it is what everything downstream
   * watches to mean "the plate changed", so setting it before the files it
   * names are registered would announce a plate that cannot yet resolve its own
   * objects. See {@link WorkplateSession}, the only caller.
   */
  adoptRestored(requestUuid: string, files: readonly WorkplateFile[], primary: File | null): void {
    this.files.set([...files]);
    this.sourceFilename.set(files[0]?.filename?.trim() || null);
    this.selectedFile.set(primary);
    this.uploadProgress.set(0);
    this.uploadError.set(null);
    this.requestUuid.set(requestUuid);
  }

  /**
   * Mark the selected file as belonging to a local-only workplate.
   *
   * `localFile` registers the model under a locally-minted handle, so
   * {@link files} is populated in web and native mode exactly as an upload
   * populates it in cloud mode. Everything downstream — the viewer's
   * `modelSourceId`, and through it each object's `source_id` — then works the
   * same way in every runtime instead of only where a server hands out ids.
   */
  adoptLocal(requestUuid: string, localFile?: WorkplateFile): void {
    this.requestUuid.set(requestUuid);
    this.files.set(localFile ? [localFile] : []);
    this.uploadProgress.set(0);
    this.uploadError.set(null);
    if (!this.sourceFilename()) {
      const selected = this.selectedFile()?.name?.trim();
      this.sourceFilename.set(selected || null);
    }
  }

  /**
   * Fetch one uploaded model by its `file_uuid`.
   *
   * Deliberately free of side effects. Restoring a plate downloads each of its
   * files in turn, and a fetch that also adopted what it fetched would announce
   * a half-built plate once per file — and leave the viewer pointed at whichever
   * model happened to come last.
   */
  fetchModel(fileUuid: string, filename: string): Promise<File> {
    this.uploadProgress.set(0);
    this.uploadError.set(null);

    return new Promise((resolve, reject) => {
      this.#http
        .get(`${environment.apiUrl}/file/${fileUuid}`, {
          responseType: 'blob',
          reportProgress: true,
          observe: 'events',
        })
        .subscribe({
          next: (event) => {
            if (event.type === HttpEventType.DownloadProgress) {
              const progress = event.total ? Math.round((event.loaded / event.total) * 100) : 0;
              this.uploadProgress.set(progress);
            } else if (event.type === HttpEventType.Response) {
              const blob = event.body;
              if (!blob || !(blob instanceof Blob)) {
                const message = 'Invalid response: expected Blob';
                this.uploadError.set(message);
                reject(new Error(message));
                return;
              }
              this.uploadProgress.set(100);
              resolve(new File([blob], filename, { type: 'application/octet-stream' }));
            }
          },
          error: (error: unknown) => {
            let message = 'Failed to load model';
            if (error instanceof Error) {
              message = error.message;
            } else if (typeof error === 'object' && error !== null && 'error' in error) {
              const err = error as { error?: { message?: string } };
              message = err.error?.message || message;
            }
            this.uploadError.set(message);
            this.uploadProgress.set(0);
            console.error('[SlicerFile] fetchModel error:', message);
            reject(new Error(message));
          },
        });
    });
  }
}
