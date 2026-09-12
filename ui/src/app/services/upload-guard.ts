import { DestroyRef, inject, Injectable } from '@angular/core';
import { takeUntilDestroyed, toObservable } from '@angular/core/rxjs-interop';
import { CanDeactivateFn } from '@angular/router';
import { Observable, fromEvent, of } from 'rxjs';
import { switchMap } from 'rxjs/operators';
import { Dialog } from './dialog';
import { SlicerFile } from './slicer-file';

/**
 * Registers a `beforeunload` listener whenever an upload is in progress,
 * prompting the browser to confirm before the tab is closed or refreshed.
 * Also exposes a `canDeactivate` guard for the router.
 */
@Injectable({ providedIn: 'root' })
export class UploadGuard {
  readonly #slicerFile = inject(SlicerFile);
  readonly #destroyRef = inject(DestroyRef);
  readonly #dialog = inject(Dialog);

  constructor() {
    // Convert the uploadProgress signal to an observable, then switch to a
    // fromEvent subscription only while an upload is active.
    toObservable(this.#slicerFile.isUploading)
      .pipe(
        switchMap((uploading) => {
          return uploading ? fromEvent<BeforeUnloadEvent>(window, 'beforeunload') : [];
        }),
        takeUntilDestroyed(this.#destroyRef),
      )
      .subscribe((event) => {
        event.preventDefault();
      });
  }

  /**
   * `window.confirm` put an unstyled OS prompt in the middle of an app that
   * ships its own dialog — and on the Tauri targets a blocking `confirm` is not
   * something the webview is guaranteed to honour at all. `Dialog` renders the
   * same question in the app's own language, and routes to a native sheet where
   * there is one.
   */
  canLeave(): Observable<boolean> {
    if (!this.#slicerFile.isUploading()) {
      return of(true);
    }

    return this.#dialog.confirm({
      title: 'Leave while uploading?',
      message: 'The upload in progress will be cancelled.',
      confirmLabel: 'Leave',
      cancelLabel: 'Stay',
      type: 'warning',
    });
  }
}

export const uploadCanDeactivate: CanDeactivateFn<unknown> = () => {
  return inject(UploadGuard).canLeave();
};
