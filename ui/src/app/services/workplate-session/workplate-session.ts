import { Injectable, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { Router } from '@angular/router';
import { resolveRuntimeMode } from '../../runtime/domain/runtime-mode.util';
import { Logger } from '../logger';
import { ModelSourceRegistry, type ModelSource } from '../model-source';
import { NotificationService } from '../notifications';
import { SceneEngine, type SceneOp } from '../scene-engine';
import { Slicer } from '../slicer';
import { SlicerConnection } from '../slicer-connection';
import { SlicerFile } from '../slicer-file';
import { WorkplateObjects } from '../workplate-objects';
import { WorkplateSettingsStore } from '../workplate-settings';
import { placementKey, planPlacements, type PlacedObject } from './placement-plan';

/**
 * One of a plate's files, resolved to something the scene engine can parse.
 *
 * The bytes travel beside the {@link ModelSource} rather than inside it because
 * only the local runtimes keep a copy: in cloud mode the server holds the model
 * and the registry deliberately stores nothing but the id and the name.
 */
interface ResolvedFile {
  source: ModelSource;
  bytes: Uint8Array;
}

/**
 * The one way a workplate becomes the plate on screen.
 *
 * Tabs made this necessary. A plate used to be opened by whoever happened to
 * navigate to it, and each caller brought its own idea of what "opening" meant:
 * the slice viewer refetched a cloud plate's models, a locally-minted plate
 * (`local-…`, which is *every* plate in the desktop, iPad and browser builds)
 * was skipped outright, and nothing at all put the objects back where the user
 * had left them. Switching tabs therefore changed the address bar and, outside
 * cloud mode, nothing else — same scene, same settings, wrong title.
 *
 * So opening a plate is one operation, in one place, and it is the same
 * operation in all four runtimes:
 *
 * 1. Flush what the outgoing plate still owes to storage.
 * 2. Fetch the incoming plate's document **and wait for it** — the scene is
 *    rebuilt from that document, and a plate rebuilt before its document
 *    arrives is a plate that silently opens with someone else's presets.
 * 3. Tear the old plate down completely ({@link Slicer.resetWorkplate}).
 * 4. Resolve every file the document names — from the server in cloud mode,
 *    from the {@link ModelSourceRegistry}'s vault everywhere else.
 * 5. Rebuild the scene: one object per record, each with the transform and the
 *    support paint it was saved with.
 * 6. Adopt the plate, which is what lets the rest of the app — the title, the
 *    settings panel, the objects list, the slice button — follow.
 *
 * **Opens are serialised.** A user clicking along a row of tabs starts an open
 * every few hundred milliseconds, and two of these interleaved would deal
 * objects from both plates into one scene.
 */
@Injectable({ providedIn: 'root' })
export class WorkplateSession {
  private readonly log = inject(Logger).scope('WorkplateSession');
  private readonly router = inject(Router);
  private readonly slicer = inject(Slicer);
  private readonly slicerFile = inject(SlicerFile);
  private readonly sceneEngine = inject(SceneEngine);
  private readonly modelSources = inject(ModelSourceRegistry);
  private readonly plates = inject(WorkplateSettingsStore);
  private readonly workplateObjects = inject(WorkplateObjects);
  private readonly notifications = inject(NotificationService);
  private readonly connection = inject(SlicerConnection);

  /** True while a plate is being brought back, for the viewport's overlay. */
  readonly restoring = signal(false);

  /**
   * Set when the engine reports that the plate on screen was changed by someone
   * else, and cleared when the user acts on it.
   *
   * Deliberately a prompt and not a reload. Two people on one plate is ordinary
   * — someone tuning settings while someone else arranges models — and pulling
   * the scene out from under whichever of them typed second is worse than
   * letting them pick the moment. Last writer still wins if they ignore it.
   */
  readonly changedElsewhere = signal<{ uuid: string; at: string | null } | null>(null);

  /** Plates with an open queued or in flight, so a repeat click is free. */
  readonly #queued = new Set<string>();
  /** The newest plate asked for; an older queued open is superseded by it. */
  #latest: string | null = null;
  /** Tail of the serialised open queue. */
  #queue: Promise<void> = Promise.resolve();

  constructor() {
    // Only relevant where there is a second client to diverge from. In the
    // native and web runtimes `messages$` is `EMPTY`, so this is inert.
    this.connection.messages$.pipe(takeUntilDestroyed()).subscribe((msg) => {
      if (msg.type !== 'WorkplateChanged') {
        return;
      }
      // Only the plate the user is actually looking at. A notice about a plate
      // in another tab is something they cannot act on from here, and a strip
      // of them is how a useful prompt becomes one people click past.
      if (msg.request_uuid === this.slicerFile.requestUuid()) {
        this.changedElsewhere.set({ uuid: msg.request_uuid, at: msg.updated_at ?? null });
      }
    });
  }

  /** Dismiss the "changed elsewhere" prompt without reloading. */
  keepMine(): void {
    this.changedElsewhere.set(null);
  }

  /**
   * Fetch the plate again from the engine, discarding what this browser holds.
   *
   * The answer to the prompt above, and the one path that deliberately reopens
   * a plate that is already open.
   */
  async refresh(uuid: string): Promise<void> {
    this.changedElsewhere.set(null);
    this.plates.forget(uuid);
    this.#latest = uuid;
    this.#queued.add(uuid);
    this.#queue = this.#queue
      .catch(() => undefined)
      .then(() => this.#open(uuid, true))
      .catch((error: unknown) => this.#reportOpenFailure(uuid, error))
      .finally(() => this.#queued.delete(uuid));
    return this.#queue;
  }

  /**
   * Make `uuid` the plate on screen, restoring everything it remembers.
   *
   * Cheap and idempotent for the plate already open — which is the common case,
   * because creating a plate navigates to it and the route then asks for the
   * very plate the upload just left loaded.
   */
  open(uuid: string): Promise<void> {
    if (this.#queued.has(uuid)) {
      return this.#queue;
    }
    if (this.#queued.size === 0 && this.slicerFile.requestUuid() === uuid) {
      return Promise.resolve();
    }
    this.#queued.add(uuid);
    this.#latest = uuid;
    this.#queue = this.#queue
      .catch(() => undefined)
      .then(() => this.#open(uuid))
      .catch((error: unknown) => this.#reportOpenFailure(uuid, error))
      .finally(() => this.#queued.delete(uuid));
    return this.#queue;
  }

  #reportOpenFailure(uuid: string, error: unknown): void {
    this.log.error(`could not open plate '${uuid}'`, String(error));
    this.notifications.error(
      'Could not open workplate',
      error instanceof Error ? error.message : undefined,
    );
  }

  /**
   * Leave the current plate for a clean slate, without opening another.
   *
   * The plate being left is not destroyed — its document and its files are
   * exactly where {@link open} will look for them — so this is "put this down",
   * not "throw this away". That distinction is what lets the `+` in the tab
   * strip stop taking the plate behind it with it.
   */
  async newPlate(): Promise<void> {
    this.#latest = null;
    this.changedElsewhere.set(null);
    this.plates.flush();
    await this.slicer.resetWorkplate();
    await this.router.navigate(['/']);
  }

  async #open(uuid: string, force = false): Promise<void> {
    // Clicking along a row of tabs queues one of these per tab. Only the last
    // one is worth doing: the others would each tear a plate down and rebuild
    // it for nobody. `force` is the deliberate exception — {@link refresh}
    // reopens the plate that is already open, on purpose.
    if (this.#latest !== uuid || (!force && this.slicerFile.requestUuid() === uuid)) {
      return;
    }
    this.restoring.set(true);
    // Nothing is recorded while the scene is half-built: the recorder watches
    // the scene, and it would write each intermediate state back over the very
    // document being read from.
    const release = this.plates.beginRestore();
    try {
      this.plates.flush();
      await this.plates.hydrate(uuid);
      const setup = this.plates.settingsFor(uuid);

      await this.slicer.resetWorkplate();

      const files = await this.#resolveFiles(uuid, setup.objects ?? []);
      // A plate saved before placements were recorded — or by a build that did
      // not record them — still has to open. Its files land on the bed the way
      // a fresh drop would rather than the plate refusing to come back.
      const records = setup.objects ?? [];
      const restored =
        records.length > 0
          ? await this.#rebuildScene(records, files)
          : await this.#placeFresh(files);

      // The viewer takes its model from `selectedFile` and matches the plate by
      // `files[0]`, so the file the first object came from has to lead. Given a
      // match against an object the engine already holds, the viewer adopts the
      // scene we just built instead of parsing the model again and placing it
      // afresh — which is what keeps the transforms.
      this.slicerFile.adoptRestored(
        uuid,
        files.map((f) => ({ fileId: f.source.sourceId, filename: f.source.fileName })),
        restored.primary,
      );

      if (restored.missing > 0) {
        this.notifications.warning(
          restored.missing === 1
            ? 'One model could not be restored'
            : `${restored.missing} models could not be restored`,
          'Their files are no longer available on this device. Add them again to put the plate back together.',
        );
      }
    } finally {
      release();
      this.restoring.set(false);
    }
  }

  /**
   * Every file the plate's objects name, as bytes this session can parse.
   *
   * Cloud asks the server, which is the copy that survived the browser being
   * cleared. Everywhere else the browser *is* the engine, so the vault behind
   * the registry is the only copy there can be — and the reason a plate now
   * survives the desktop app quitting or iPadOS reclaiming the webview.
   */
  async #resolveFiles(uuid: string, objects: readonly PlacedObject[]): Promise<ResolvedFile[]> {
    if (resolveRuntimeMode() === 'cloud') {
      return this.#downloadFromServer(uuid);
    }
    const ids = [...new Set(objects.map((o) => o.file_id).filter(Boolean))];
    await this.modelSources.hydrate(ids);
    return ids
      .map((id) => this.modelSources.get(id))
      .filter((source): source is ModelSource => !!source?.bytes)
      .map((source) => ({ source, bytes: source.bytes as Uint8Array }));
  }

  /** Pull the plate's uploads back down and register each under its own id. */
  async #downloadFromServer(uuid: string): Promise<ResolvedFile[]> {
    const meta = await this.slicerFile.getRequestMeta(uuid);
    const files: ResolvedFile[] = [];
    for (const entry of meta.ofids) {
      try {
        const file = await this.slicerFile.fetchModel(entry.file_uuid, entry.original_filename);
        files.push({
          // No bytes on the record: the server holds the model and resolves the
          // id at slice time, so a second copy in the tab buys nothing and costs
          // the size of every model the plate holds.
          source: this.modelSources.register({
            sourceId: entry.file_uuid,
            fileName: entry.original_filename,
          }),
          bytes: new Uint8Array(await file.arrayBuffer()),
        });
      } catch (error) {
        this.log.warn(`could not fetch '${entry.original_filename}'`, String(error));
      }
    }
    return files;
  }

  /**
   * Put the saved objects back, one scene object per record.
   *
   * Each file is parsed **once**, however many objects came out of it: a 3MF
   * yields one id per part, and a model the user duplicated is duplicated again
   * here rather than re-parsed. Which record gets which object is decided up
   * front by {@link planPlacements}, so the rule — including what to do with a
   * part the document never claimed — is testable without a scene engine.
   */
  async #rebuildScene(
    objects: readonly PlacedObject[],
    files: readonly ResolvedFile[],
  ): Promise<{ missing: number; primary: File | null }> {
    if (objects.length === 0) {
      return { missing: 0, primary: null };
    }
    await this.sceneEngine.ready();

    /** The object each file+part parsed into, before any duplication. */
    const parsed = new Map<string, bigint>();
    const partsByFile = new Map<string, number>();
    for (const { source, bytes } of files) {
      const ids = this.sceneEngine.addMesh(source.fileName, source.format, bytes, source.sourceId);
      partsByFile.set(source.sourceId, ids.length);
      ids.forEach((id, part) => parsed.set(placementKey(source.sourceId, part), id));
    }

    const plan = planPlacements(objects, partsByFile);
    const byId = new Map(files.map((file) => [file.source.sourceId, file]));
    const ops: SceneOp[] = [];
    let missing = plan.missing;
    let primary: File | null = null;

    for (const placement of plan.placements) {
      const template = parsed.get(placement.key);
      const id = placement.occurrence === 0 ? template : this.#duplicate(template);
      if (id === undefined) {
        missing += 1;
        continue;
      }
      const transform = placement.record.transform;
      if (transform?.translation && transform.euler_xyz_deg && transform.scale) {
        ops.push({
          op: 'SetTransform',
          args: {
            id,
            translation: transform.translation,
            euler_xyz_deg: transform.euler_xyz_deg,
            scale: transform.scale,
          },
        });
      }
      if (placement.record.support_paint) {
        ops.push({
          op: 'SetSupportPaint',
          args: { id, encoded: placement.record.support_paint },
        });
      }
      const file = byId.get(placement.record.file_id);
      if (!primary && file) {
        primary = new File([file.bytes as BlobPart], file.source.fileName);
      }
    }

    for (const key of plan.spare) {
      const id = parsed.get(key);
      if (id !== undefined) {
        ops.push({ op: 'Remove', args: { id } });
      }
    }
    if (ops.length > 0) {
      this.sceneEngine.applyBatch(ops);
    }
    return { missing, primary };
  }

  /** Drop each file onto the bed as a fresh add — the legacy-plate path. */
  async #placeFresh(
    files: readonly ResolvedFile[],
  ): Promise<{ missing: number; primary: File | null }> {
    let primary: File | null = null;
    for (const { source, bytes } of files) {
      const file = new File([bytes as BlobPart], source.fileName);
      primary ??= file;
      await this.workplateObjects.addUploadedFile(file, source.sourceId);
    }
    return { missing: 0, primary };
  }

  /**
   * Place a second copy of an object the plate holds more than once.
   *
   * `Duplicate` does not report the id it minted, so it is read off the scene —
   * cheaper than parsing a model the engine has already loaded once, which is
   * what a plate holding ten copies of one part would otherwise cost.
   */
  #duplicate(templateId: bigint | undefined): bigint | undefined {
    if (templateId === undefined) {
      return undefined;
    }
    const before = new Set(this.sceneEngine.objects().map((o) => o.id));
    this.sceneEngine.apply({ op: 'Duplicate', args: { id: templateId, offset: [0, 0, 0] } });
    return this.sceneEngine.objects().find((o) => !before.has(o.id))?.id;
  }
}
