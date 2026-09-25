import { Injectable, computed, inject, signal } from '@angular/core';
import { resolveRuntimeMode } from '../../runtime/domain/runtime-mode.util';
import { onIdle } from '../idle';
import { Logger } from '../logger';
import { NotificationService } from '../notifications';
import { SceneEngine } from '../scene-engine';
import { BrowserLibraryBackend } from './browser-library-backend';
import {
  NativeLibraryBackend,
  RemoteLibraryBackend,
  type ImportOutcome,
  type ImportResult,
  type Library,
  type LibraryBackend,
  type LibraryEntry,
  type LibrarySettings,
} from './library-backend';

function createBackend(): LibraryBackend {
  switch (resolveRuntimeMode()) {
    case 'native':
      return new NativeLibraryBackend();
    case 'cloud':
      return new RemoteLibraryBackend();
    default:
      return new BrowserLibraryBackend();
  }
}

/** Whether a {@link ImportResult} failed. */
export function isImportError(result: ImportResult): result is { error: string } {
  return 'error' in result;
}

/**
 * The object library: every model that has reached a plate, once, with a
 * picture of it.
 *
 * The engine owns the document and the matching (see `src/library/`); this
 * service is the webview's view of it plus the one thing only the webview can
 * do — **draw the thumbnails**. The engine has no renderer and must not grow
 * one, so a model with no picture is rendered here, once, and the PNG handed
 * back to be kept beside the entry.
 *
 * Why pictures and not a grid of live 3D views: a browser allows a handful of
 * WebGL contexts, and a live view has to hold the whole model in memory. A
 * library of a few hundred files would exhaust the first and the second on an
 * iPad long before the user scrolled to the bottom. A thumbnail is drawn once,
 * costs tens of kilobytes forever after, and the one model the user selects
 * still gets a live, turnable view.
 *
 * Recording is fire and forget: {@link remember} is called from every path a
 * model reaches a plate by, and none of them waits on it.
 */
@Injectable({ providedIn: 'root' })
export class ObjectLibrary {
  readonly #log = inject(Logger).scope('ObjectLibrary');
  readonly #sceneEngine = inject(SceneEngine);
  readonly #notifications = inject(NotificationService);
  readonly #backend = createBackend();

  /** What this runtime's library can do. */
  readonly capabilities = this.#backend.capabilities;

  readonly #library = signal<Library | null>(null);
  readonly #loading = signal(false);
  readonly #thumbnails = signal<ReadonlyMap<string, string>>(new Map());
  readonly #rendering = signal<ReadonlySet<string>>(new Set());

  /** `true` until the library has been read once. */
  readonly loaded = computed(() => this.#library() !== null);
  readonly loading = this.#loading.asReadonly();
  readonly settings = computed<LibrarySettings>(
    () => this.#library()?.settings ?? { mode: 'copy', folders: [] },
  );
  readonly entries = computed<readonly LibraryEntry[]>(() => this.#library()?.entries ?? []);
  /** Object URLs of every thumbnail loaded so far, by entry id. */
  readonly thumbnails = this.#thumbnails.asReadonly();
  /** Entries whose thumbnail is being drawn right now. */
  readonly rendering = this.#rendering.asReadonly();

  /** Serialises thumbnail renders — they share one WebGL renderer. */
  #renderQueue: Promise<void> = Promise.resolve();
  /** Entries a thumbnail was already requested for this session. */
  readonly #requested = new Set<string>();

  /**
   * Read the library, first picking up anything dropped into its folders —
   * on an iPad, a model saved into Files shows up here without an import.
   */
  async refresh(options: { scan?: boolean } = {}): Promise<void> {
    this.#loading.set(true);
    try {
      if (options.scan ?? (this.capabilities.visibleFolder || this.capabilities.watchFolders)) {
        await this.#backend.scan().catch((error) => this.#log.warn('scan failed', String(error)));
      }
      this.#library.set(await this.#backend.load());
    } catch (error) {
      this.#log.error('could not load the library', String(error));
      this.#library.set(
        this.#library() ?? { settings: { mode: 'copy', folders: [] }, entries: [] },
      );
    } finally {
      this.#loading.set(false);
    }
  }

  /**
   * Record a model that just reached a plate. Never rejects, never blocks —
   * the plate is what the user is waiting on.
   */
  remember(file: File): void {
    onIdle(() => void this.#remember(file));
  }

  async #remember(file: File): Promise<void> {
    try {
      await this.#sceneEngine.ready();
      const outcome = await this.#backend.remember(file);
      if (!outcome) {
        return;
      }
      if (this.loaded()) {
        await this.refresh({ scan: false });
      }
      const entry = this.entries().find((e) => e.id === outcome.entry_id);
      if (!entry?.has_thumbnail) {
        this.#queueRender(outcome.entry_id, async () => file);
      }
    } catch (error) {
      this.#log.warn(`could not record '${file.name}'`, String(error));
    }
  }

  /** Add models to the library without putting them on a plate. */
  async importFiles(files: readonly File[]): Promise<void> {
    const notifId = this.#notifications.task(
      files.length === 1 ? 'Adding to library…' : `Adding ${files.length} models to library…`,
      files.map((f) => f.name).join(', '),
    );
    await this.#sceneEngine.ready();
    const results = await this.#backend.importFiles(files);
    await this.refresh({ scan: false });

    const added = results.filter((r): r is ImportOutcome => !isImportError(r));
    const already = added.filter((r) => r.match !== 'new').length;
    const failed = results.filter(isImportError);
    if (added.length === 0) {
      this.#notifications.resolveTask(
        notifId,
        'danger',
        'Could not add to library',
        failed[0]?.error ?? 'Use an STL, OBJ or 3MF model.',
      );
      return;
    }
    this.#notifications.resolveTask(
      notifId,
      'success',
      added.length === 1 ? 'Added to library' : `${added.length} models added to library`,
      already > 0
        ? `${already} ${already === 1 ? 'was' : 'were'} already there, so no second copy was kept.`
        : undefined,
    );
    for (const [index, result] of results.entries()) {
      if (isImportError(result)) {
        this.#notifications.error(`Could not add ${files[index].name}`, result.error);
      } else {
        this.#queueRender(result.entry_id, async () => files[index]);
      }
    }
  }

  async saveSettings(settings: LibrarySettings): Promise<void> {
    this.#library.set(await this.#backend.saveSettings(settings));
    if (settings.folders?.length) {
      await this.refresh();
    }
  }

  async rename(entry: LibraryEntry, name: string): Promise<void> {
    await this.#backend.rename(entry.id, name);
    this.#patch(entry.id, { name: name.trim() || entry.name });
  }

  async remove(entry: LibraryEntry): Promise<void> {
    await this.#backend.remove(entry.id);
    this.#library.update((library) =>
      library
        ? { ...library, entries: library.entries?.filter((e) => e.id !== entry.id) }
        : library,
    );
    const url = this.#thumbnails().get(entry.id);
    if (url) {
      URL.revokeObjectURL(url);
      this.#setThumbnail(entry.id, null);
    }
  }

  /**
   * The entry's model, ready for the plate flows — and for the preview, which
   * parses it with the engine's loaders, so the engine is ready first.
   */
  async open(entry: LibraryEntry): Promise<File | null> {
    await this.#sceneEngine.ready();
    return this.#backend.open(entry);
  }

  /** Count a use — called when an entry is put on a plate. */
  async touch(entry: LibraryEntry): Promise<void> {
    await this.#backend.touch(entry.id).catch(() => undefined);
    this.#patch(entry.id, {
      use_count: (entry.use_count ?? 0) + 1,
      last_used_at: new Date().toISOString(),
    });
  }

  /** Where the library keeps its copies, when that is a folder on disk. */
  modelsDir(): Promise<string | null> {
    return this.#backend.modelsDir().catch(() => null);
  }

  /**
   * Show one of an entry's files in Finder or Explorer — `path`, or the one it
   * is read from when none is named.
   */
  async reveal(entry: LibraryEntry, path?: string): Promise<void> {
    const target = path ?? entry.locations?.find((l) => !l.missing)?.path;
    if (!target) {
      return;
    }
    try {
      await this.#backend.reveal(target);
    } catch (error) {
      this.#notifications.error(`Could not show ${entry.name}`, String(error));
    }
  }

  /**
   * Make sure an entry's picture is on its way: load the stored one, or draw
   * it if there is none. Cheap to call repeatedly; each entry is asked once.
   */
  ensureThumbnail(entry: LibraryEntry): void {
    if (this.#requested.has(entry.id) || this.#thumbnails().has(entry.id)) {
      return;
    }
    this.#requested.add(entry.id);
    if (entry.has_thumbnail) {
      void this.#backend
        .thumbnail(entry.id)
        .then((blob) => {
          if (blob) {
            this.#setThumbnail(entry.id, URL.createObjectURL(blob));
          } else {
            this.#queueRender(entry.id, () => this.#backend.open(entry));
          }
        })
        .catch(() => undefined);
    } else if (entry.locations?.some((l) => !l.missing)) {
      this.#queueRender(entry.id, () => this.#backend.open(entry));
    }
  }

  #queueRender(id: string, source: () => Promise<File | null>): void {
    this.#requested.add(id);
    this.#rendering.update((set) => new Set(set).add(id));
    this.#renderQueue = this.#renderQueue.then(async () => {
      try {
        const file = await source();
        if (!file) {
          return;
        }
        await this.#sceneEngine.ready();
        const { renderThumbnail } = await import('./library-geometry');
        const png = await renderThumbnail(file.name, new Uint8Array(await file.arrayBuffer()));
        this.#setThumbnail(id, URL.createObjectURL(png));
        await this.#backend.setThumbnail(id, png);
        this.#patch(id, { has_thumbnail: true });
      } catch (error) {
        this.#log.warn(`could not draw a thumbnail for ${id}`, String(error));
      } finally {
        this.#rendering.update((set) => {
          const next = new Set(set);
          next.delete(id);
          return next;
        });
      }
    });
  }

  #setThumbnail(id: string, url: string | null): void {
    this.#thumbnails.update((map) => {
      const next = new Map(map);
      const previous = next.get(id);
      if (previous && previous !== url) {
        URL.revokeObjectURL(previous);
      }
      if (url) {
        next.set(id, url);
      } else {
        next.delete(id);
      }
      return next;
    });
  }

  #patch(id: string, patch: Partial<LibraryEntry>): void {
    this.#library.update((library) =>
      library
        ? {
            ...library,
            entries: library.entries?.map((e) => (e.id === id ? { ...e, ...patch } : e)),
          }
        : library,
    );
  }
}
