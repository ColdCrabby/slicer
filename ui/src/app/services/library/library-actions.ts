import { Injectable, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import type { ContextMenuItem } from '../context-menu/context-menu.model';
import { NotificationService } from '../notifications';
import { OpenWorkplates } from '../open-workplates';
import { Slicer } from '../slicer';
import { SlicerFile } from '../slicer-file';
import { WorkplateNames } from '../workplate-names';
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
  readonly #openWorkplates = inject(OpenWorkplates);
  readonly #names = inject(WorkplateNames);

  /** The open plate, if there is one to add to. */
  readonly openPlate = this.#slicerFile.requestUuid;

  /**
   * Every workplate open as a tab, by the name its tab shows — with the model's
   * filename as a second line only once the name has stopped being it.
   */
  readonly workplates = computed(() =>
    this.#openWorkplates.tabs().map((tab) => {
      const name = this.#names.displayNameFor(tab.uuid, tab.filename);
      const stem = tab.filename?.replace(/\.[^.]+$/, '');
      return { uuid: tab.uuid, name, filename: stem === name ? null : tab.filename };
    }),
  );

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
    await this.addToWorkplate(entry, plate);
  }

  /**
   * Add this model to a particular open workplate. With that workplate on
   * screen it goes straight into the scene; otherwise it is parked and added
   * once the workplate's scene is loaded, as extra dropped files are.
   */
  async addToWorkplate(entry: LibraryEntry, uuid: string): Promise<void> {
    await this.#withFile(entry, async (file) => {
      if (this.#router.url.startsWith(`/slice/${uuid}`)) {
        await this.#workplate.addFilesWithFeedback([file]);
      } else {
        this.#workplate.queuePending([file]);
        await this.#router.navigate(['/slice', uuid]);
      }
    });
  }

  /**
   * The workplates to choose from, as menu rows: one per open workplate, then
   * — unless the menu already offers it beside this one — a new one. Shared by
   * the inspector's button and the card menu.
   */
  workplateMenu(entry: LibraryEntry, { withNew = true } = {}): ContextMenuItem[] {
    const rows: ContextMenuItem[] = this.workplates().map((plate) => ({
      label: plate.name,
      icon: 'cube',
      action: () => void this.addToWorkplate(entry, plate.uuid),
    }));
    if (!withNew) {
      return rows;
    }
    if (rows.length > 0) {
      rows.push({ label: '', separator: true });
    }
    rows.push({
      label: 'New workplate',
      icon: 'page-plus',
      action: () => void this.openAsPlate(entry),
    });
    return rows;
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
