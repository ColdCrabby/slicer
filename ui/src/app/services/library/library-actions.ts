import { Injectable, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { NotificationService } from '../notifications';
import { Slicer } from '../slicer';
import { SlicerFile } from '../slicer-file';
import { WorkplateObjects } from '../workplate-objects';
import type { LibraryEntry } from './library-backend';
import { ObjectLibrary } from './object-library';

/**
 * What can be done *with* a library entry — putting it on a plate.
 *
 * Shared by the full page and the flyout over the plate, so a model reaches a
 * plate the same way from both. Deliberately not exported from the library's
 * `index.ts`: `Slicer` imports that barrel, and this imports `Slicer`.
 */
@Injectable({ providedIn: 'root' })
export class LibraryActions {
  readonly #library = inject(ObjectLibrary);
  readonly #slicer = inject(Slicer);
  readonly #slicerFile = inject(SlicerFile);
  readonly #workplate = inject(WorkplateObjects);
  readonly #router = inject(Router);
  readonly #notifications = inject(NotificationService);

  /** The open plate, if there is one to add to. */
  readonly openPlate = this.#slicerFile.requestUuid;

  readonly #busy = signal(false);
  /** A model is being read off disk for a plate. */
  readonly busy = this.#busy.asReadonly();

  /** Start a new plate with this model on it. */
  async openAsPlate(entry: LibraryEntry): Promise<void> {
    await this.#withFile(entry, async (file) => {
      const started = await this.#slicer.startWorkplate(file);
      await this.#router.navigate(['/slice', started.requestUuid], {
        state: started.uploadMeta ? { uploadMeta: started.uploadMeta } : undefined,
      });
    });
  }

  /**
   * Add this model to the open plate, or start one when none is open.
   *
   * With the plate on screen the model goes straight into its scene. From
   * anywhere else it is parked the way extra dropped files are and added once
   * the plate's scene is back — adding it now would be undone by the plate
   * reloading.
   */
  async addToPlate(entry: LibraryEntry): Promise<void> {
    const plate = this.openPlate();
    if (!plate) {
      return this.openAsPlate(entry);
    }
    await this.#withFile(entry, async (file) => {
      if (this.#router.url.startsWith(`/slice/${plate}`)) {
        await this.#workplate.addFilesWithFeedback([file]);
      } else {
        this.#workplate.queuePending([file]);
        await this.#router.navigate(['/slice', plate]);
      }
    });
  }

  async #withFile(entry: LibraryEntry, use: (file: File) => Promise<void>): Promise<void> {
    if (this.#busy()) {
      return;
    }
    this.#busy.set(true);
    try {
      const file = await this.#library.open(entry);
      if (!file) {
        this.#notifications.error(
          `${entry.name} could not be opened`,
          'Its file has been moved or deleted.',
        );
        return;
      }
      await this.#library.touch(entry);
      await use(file);
    } catch (error) {
      this.#notifications.error(
        `${entry.name} could not be opened`,
        error instanceof Error ? error.message : undefined,
      );
    } finally {
      this.#busy.set(false);
    }
  }
}
