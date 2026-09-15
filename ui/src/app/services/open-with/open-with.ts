import { Injectable, inject } from '@angular/core';
import { Router } from '@angular/router';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { readFile } from '@tauri-apps/plugin-fs';

import { Logger } from '../logger';
import { NotificationService } from '../notifications';
import { Slicer } from '../slicer';
import { WorkplateObjects } from '../workplate-objects';

/** One model the OS handed to the app, as `src/open_with.rs` reports it. */
interface OpenedFile {
  path: string;
  file_name: string;
}

/** Event the native shell emits for models opened while the app is running. */
const OPENED_EVENT = 'open-with://files';

/**
 * Models that reached the app through the operating system rather than through
 * its own UI — "Open with Cold Crabby", a double-clicked `.3mf`, a share sheet
 * from Shapr3D.
 *
 * The native half ([`open_with.rs`](../../../../../ui-desktop/src-tauri/src/open_with.rs))
 * has already turned a Windows `argv`, a macOS Apple Event and an iOS
 * security-scoped document into the same thing: a readable path.
 *
 * **A model handed over by the OS always opens a fresh plate**, whatever is
 * already on screen. The tap that started this happened in another app, with
 * no view of the arrangement here, so it can only ever mean "open this
 * model" — never "add this to whatever I was doing". Joining an existing
 * plate is what dropping a file onto the window itself is for.
 *
 * This service is reached through a dynamic `import()` in
 * [`App`](../../app.ts), so neither it nor the slicing runtime behind
 * {@link Slicer} is in the initial download — on the web nothing here is ever
 * fetched at all.
 */
@Injectable({ providedIn: 'root' })
export class OpenWith {
  readonly #log = inject(Logger).scope('OpenWith');
  readonly #router = inject(Router);
  readonly #notifications = inject(NotificationService);
  readonly #slicer = inject(Slicer);
  readonly #workplate = inject(WorkplateObjects);

  /**
   * Serialises arrivals, so two files in quick succession cannot race the plate
   * the first of them is still creating.
   */
  #queue: Promise<void> = Promise.resolve();
  #started = false;

  /**
   * Begin accepting models from the OS. Idempotent.
   *
   * Two routes, both needed: a cold launch delivers the file to the shell
   * before the webview exists, so the buffer is drained once on connect, and
   * anything later arrives as an event.
   */
  start(): void {
    if (this.#started) {
      return;
    }
    this.#started = true;
    void this.#connect();
  }

  async #connect(): Promise<void> {
    try {
      await listen<OpenedFile[]>(OPENED_EVENT, (event) => this.#enqueue(event.payload));
      // Only now that nothing can be missed: whatever the launch itself carried.
      this.#enqueue(await invoke<OpenedFile[]>('take_opened_files'));
    } catch (error) {
      this.#log.error('could not subscribe to opened files', String(error));
    }
  }

  #enqueue(files: readonly OpenedFile[]): void {
    if (files.length === 0) {
      return;
    }
    this.#queue = this.#queue.then(() => this.#plate(files)).catch(() => undefined);
  }

  async #plate(files: readonly OpenedFile[]): Promise<void> {
    const notifId = this.#notifications.progress(
      files.length === 1 ? 'Opening model…' : `Opening ${files.length} models…`,
      files.map((f) => f.file_name).join(', '),
    );
    try {
      const models = await Promise.all(files.map((file) => this.#read(file)));

      const [first, ...rest] = models;
      const started = await this.#slicer.startWorkplate(first);
      // Queued only after the plate exists — `startWorkplate` resets the scene.
      this.#workplate.queuePending(rest);
      await this.#router.navigate(['/slice', started.requestUuid], {
        state: started.uploadMeta ? { uploadMeta: started.uploadMeta } : undefined,
      });
      this.#notifications.completeProgress(notifId, 'Model opened', first.name);
    } catch (error) {
      const message = error instanceof Error ? error.message : undefined;
      this.#log.error('could not open model', message ?? String(error));
      this.#notifications.failProgress(notifId, 'Could not open model', message);
    }
  }

  /**
   * Read an opened model into a `File` that still knows where it came from.
   *
   * The path rides along on the `File` so the native slicer reads the model off
   * disk rather than caching a second copy of bytes it was just handed — the
   * same optimisation a native file-picker selection gets.
   */
  async #read(file: OpenedFile): Promise<File> {
    const bytes = await readFile(file.path);
    const model = new File([bytes as BlobPart], file.file_name);
    Object.defineProperty(model, 'path', { value: file.path });
    return model;
  }
}
