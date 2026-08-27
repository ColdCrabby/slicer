import { save } from '@tauri-apps/plugin-dialog';
import { writeFile } from '@tauri-apps/plugin-fs';
import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
import { environment } from '../../environments/environment';
import { SlicingParams } from '../../generated/slicer-engine-ws-client-message-v1';
import type { ProfileSelection } from '../../generated/slicer-engine-ws-client-message-v1';
import { DEFAULT_SETTINGS } from '../models/slice-settings.model';
import { RuntimeOrchestrator } from '../runtime/application/runtime-orchestrator';
import { RuntimeSession } from '../runtime/application/runtime-session';
import { RuntimeHistorySession } from '../runtime/domain/history-models';
import { RuntimeMode } from '../runtime/domain/runtime-mode';
import { RuntimeMeshInput, RuntimeSceneSnapshot } from '../runtime/domain/scene-commands';
import { createRuntime } from '../runtime/factory/runtime-factory';
import { RuntimeEvent } from '../runtime/ports/runtime-events';
import { NotificationService } from './notifications';
import { ActiveSelection } from './profiles/active-selection';
import { SceneEngine } from './scene-engine';
import { AppVersion } from './app-version';
import { ConnectionStatus, SlicerConnection } from './slicer-connection';
import { SlicerFile, UploadResponse } from './slicer-file';
import {
  ViewerControl,
  type ThumbnailColorMode,
  type ThumbnailTheme,
  type ThumbnailView,
} from './viewer-control';
import { WorkplateNames } from './workplate-names';

/** Human-readable label for each pipeline phase. */
export const PHASE_LABELS: Record<string, string> = {
  total: 'Slicing',
  mesh_load: 'Loading mesh',
  mesh_analysis: 'Analysing mesh',
  slicing: 'Slicing layers',
  wall_generation: 'Generating walls',
  infill_region_snapshot: 'Mapping infill regions',
  wall_restrictions: 'Applying wall restrictions',
  interior_regions: 'Computing interior regions',
  wall_top_detect: 'Detecting top surfaces',
  wall_apply: 'Refining walls',
  surfaces: 'Generating surfaces',
  'Overhang Perimeter Classification': 'Classifying overhangs',
  infill: 'Generating infill',
  'Path Ordering': 'Ordering travel paths',
  'Flow Compensation': 'Compensating flow',
  gcode_generation: 'Generating G-code',
  file_write: 'Writing output',
};

/**
 * Format a millisecond duration as a compact, human-friendly string:
 * `940` → `0.9 s`, `2519` → `2.5 s`, `72500` → `1 m 12 s`.
 */
export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return '';
  if (ms < 1000) return `${Math.round(ms)} ms`;
  const totalSeconds = ms / 1000;
  if (totalSeconds < 60) return `${totalSeconds.toFixed(1)} s`;
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = Math.round(totalSeconds - minutes * 60);
  return `${minutes} m ${seconds} s`;
}

/**
 * Proportional weights per phase derived from typical Benchy timings.
 * `total` is the outer span and excluded from progress accumulation.
 */
const PHASE_WEIGHTS: Record<string, number> = {
  mesh_load: 6,
  mesh_analysis: 1,
  slicing: 46,
  wall_generation: 11,
  infill_region_snapshot: 4,
  wall_restrictions: 7,
  interior_regions: 4,
  surfaces: 8,
  infill: 2,
  gcode_generation: 13,
  file_write: 1,
};
const PHASE_TOTAL_WEIGHT = Object.values(PHASE_WEIGHTS).reduce((a, b) => a + b, 0);

/** Maximum time (ms) to wait for a slice operation before timing out. */
const SLICE_TIMEOUT_MS = 30 * 60 * 1000; // 30 minutes
const DEFAULT_THUMBNAIL_SIZE_PX = 320;

export type SlicerStatus = 'idle' | 'ready' | 'uploading' | 'slicing' | 'done' | 'error';

export interface PhaseTimingData {
  phase: string;
  startTime?: number;
  endTime?: number;
  elapsedMs?: number;
}

export interface WorkplateStart {
  requestUuid: string;
  uploadMeta?: UploadResponse;
}

@Injectable({ providedIn: 'root' })
export class Slicer {
  private readonly wsConnection = inject(SlicerConnection);
  private readonly slicerFile = inject(SlicerFile);
  private readonly notifications = inject(NotificationService);
  private readonly sceneEngine = inject(SceneEngine);
  private readonly activeSelection = inject(ActiveSelection);
  private readonly appVersion = inject(AppVersion);
  private readonly viewerControl = inject(ViewerControl);
  private readonly workplateNames = inject(WorkplateNames);
  private readonly runtimeMode = this.resolveRuntimeMode();
  private readonly runtime = createRuntime({
    mode: this.runtimeMode,
    apiUrl: environment.apiUrl,
    wsUrl: environment.wsUrl,
    sceneEngine: this.sceneEngine,
    slicerConnection: this.wsConnection,
    slicerFile: this.slicerFile,
  });
  private readonly runtimeSession = new RuntimeSession(this.runtimeMode);
  private readonly orchestrator = new RuntimeOrchestrator(this.runtime, this.runtimeSession);
  private sliceAbort: AbortController | null = null;
  private activeSliceId: string | null = null;
  /** Cached mesh input from a native file-picker selection. When set,
   *  `readRuntimeMeshInput` returns this directly (avoiding `arrayBuffer()`
   *  on the File object) and the `filePath` field enables path-only IPC. */
  private pendingNativeMeshInput: RuntimeMeshInput | null = null;

  /**
   * Currently-selected file. Sourced from {@link SlicerFile} so the upload
   * page and the viewer page share a single source of truth.
   */
  readonly selectedFile = this.slicerFile.selectedFile;
  /** Workplate UUID of the current scene (the key sliced G-code is stored under). */
  readonly currentRequestUuid = this.slicerFile.requestUuid;
  readonly settings = signal<SlicingParams>(DEFAULT_SETTINGS);
  readonly status = signal<SlicerStatus>('idle');
  readonly runtimeConnected = signal(false);
  readonly historyVersion = signal(0);
  readonly historyReady = computed<boolean>(() => {
    if (this.runtimeMode === 'cloud') {
      return this.wsConnection.isConnected();
    }
    return this.runtimeConnected();
  });
  readonly connectionStatus = computed<ConnectionStatus>(() => {
    if (this.runtimeMode === 'cloud') {
      return this.wsConnection.status();
    }

    // For now, non-cloud runtimes are treated as always connected from the UI's
    // perspective. Runtime init errors still surface through `status` + logs.
    return 'connected';
  });
  readonly shouldShowConnectionStatus = computed(() => this.runtimeMode === 'cloud');
  readonly outputLog = signal<string[]>([]);
  readonly phaseTimings = signal<PhaseTimingData[]>([]);

  /** Resolved download URL for the last completed slice, or `null` when none. */
  readonly gcodeDownloadUrl = signal<string | null>(null);

  /**
   * Object IDs (stringified) of the scene captured for the most recent slice.
   * Consumers compare successive sets to tell a resliced scene (shared ids)
   * from a brand-new one (fully disjoint ids).
   */
  readonly slicedObjectIds = signal<readonly string[]>([]);

  /**
   * Signature of the scene + settings at the moment the current preview was
   * sliced. `null` until the first slice; compared against the live signature
   * to detect when the on-screen G-code no longer matches the main scene.
   */
  private readonly slicedSignature = signal<string | null>(null);

  /**
   * Live signature of the scene (object ids + placement) and slice settings.
   * Recomputes reactively whenever an object moves or a setting changes.
   */
  private readonly sceneSignature = computed(() => {
    const objects = this.sceneEngine
      .snapshot()
      .objects.map(
        (o) =>
          `${String(o.id)}@${o.translation.join(',')}/${o.euler_xyz_deg.join(',')}/${o.scale.join(',')}`,
      )
      .sort()
      .join('|');
    return `${objects}#${JSON.stringify(this.settings())}`;
  });

  /**
   * `true` when a preview exists but the scene or settings changed since it was
   * sliced — the on-screen G-code is stale. Non-blocking: it only hints that a
   * re-slice would refresh the preview.
   */
  readonly previewStale = computed(() => {
    const sliced = this.slicedSignature();
    if (sliced === null || this.gcodeDownloadUrl() === null) {
      return false;
    }
    return this.sceneSignature() !== sliced;
  });

  /** Name of the pipeline phase currently executing, or `null` when idle. */
  readonly currentPhase = signal<string | null>(null);
  private objectUrl: string | null = null;

  /**
   * The workplate uuid the transient slice state currently belongs to.
   * `undefined` until the first observation so the initial value is adopted
   * without flushing anything. A change to any *other* value means the active
   * plate switched (new upload, opened history entry, deep-link) and the
   * previous plate's slice results must be discarded.
   */
  private lastWorkplateUuid: string | null | undefined = undefined;

  /**
   * Overall slice progress 0–100.
   *
   * - When `status === 'done'`, always returns 100.
   * - Each phase has a known proportional weight. Completed phases (those with
   *   an `endTime`) are stacked in order; their cumulative weight over the
   *   total determines the percentage.
   * - Capped at 99 until `SliceComplete` arrives to avoid a premature 100%.
   */
  readonly sliceProgress = computed(() => {
    if (this.status() === 'done') return 100;

    const timings = this.phaseTimings();
    let completedWeight = 0;

    for (const t of timings) {
      if (t.endTime != null && t.phase !== 'total' && PHASE_WEIGHTS[t.phase] != null) {
        completedWeight += PHASE_WEIGHTS[t.phase];
      }
    }

    return Math.min(99, Math.round((completedWeight / PHASE_TOTAL_WEIGHT) * 100));
  });

  /**
   * Wall-clock duration (ms) of the last completed slice, measured client-side
   * from job start to completion. This is the runtime-agnostic source of truth:
   * the web/wasm and tauri runtimes do not emit a `total` phase span, so the
   * per-phase timings alone cannot yield an overall time.
   */
  readonly lastSliceElapsedMs = signal<number | null>(null);

  /** Timestamp (performance.now) when the active slice job began. */
  private sliceStartedAt: number | null = null;

  /**
   * Total time of the last completed slice, in milliseconds. Prefers the
   * backend `total` phase span when the runtime reports one (the cloud/server
   * path, which excludes client/network overhead) and otherwise falls back to
   * the client-measured {@link lastSliceElapsedMs}.
   */
  readonly totalElapsedMs = computed<number | null>(() => {
    const total = this.phaseTimings().find((t) => t.phase === 'total');
    if (total?.elapsedMs && total.elapsedMs > 0) return total.elapsedMs;
    return this.lastSliceElapsedMs();
  });

  constructor() {
    this.orchestrator.onEvent((event) => this.handleRuntimeEvent(event));
    this.orchestrator.init().catch((error) => {
      this.runtimeConnected.set(false);
      this.status.set('error');
      this.outputLog.update((log) => [
        ...log,
        `[error] Runtime initialization failed: ${error instanceof Error ? error.message : String(error)}`,
      ]);
    });

    // Forget the previous plate's slice the instant the active workplate
    // changes. `startWorkplate` / `resetWorkplate` also flush imperatively,
    // but opening a history entry or deep-linking to `/slice/:uuid` swaps the
    // workplate without going through them — this reactive guard keeps the
    // download URL, progress rail, thumbnail source and view mode from leaking
    // across plates in those paths too. A reslice keeps the same uuid, so it
    // never fires mid-job.
    effect(() => {
      const uuid = this.currentRequestUuid();
      untracked(() => {
        if (this.lastWorkplateUuid === undefined) {
          this.lastWorkplateUuid = uuid;
          return;
        }
        if (uuid === this.lastWorkplateUuid) {
          return;
        }
        this.lastWorkplateUuid = uuid;
        this.clearSliceState();
        this.viewerControl.viewMode.set('model');
      });
    });
  }

  private handlePhaseMarker(msg: {
    phase: string;
    event: string;
    elapsed_ms?: number | null;
  }): void {
    const now = Date.now();

    if (msg.event === 'start') {
      // Phase started - track as the current active phase and add timing entry
      if (msg.phase !== 'total') {
        this.currentPhase.set(msg.phase);
      }
      this.phaseTimings.update((timings) => {
        const existing = timings.find((t) => t.phase === msg.phase);
        if (existing) {
          existing.startTime = now;
          existing.endTime = undefined;
          existing.elapsedMs = undefined;
          return [...timings];
        } else {
          return [...timings, { phase: msg.phase, startTime: now }];
        }
      });
      this.outputLog.update((l) => [...l, `[phase] ${msg.phase} → start`]);
    } else if (msg.event === 'end' && msg.elapsed_ms != null) {
      // Phase ended - update with elapsed time
      this.phaseTimings.update((timings) => {
        const existing = timings.find((t) => t.phase === msg.phase);
        if (existing) {
          existing.endTime = now;
          existing.elapsedMs = msg.elapsed_ms ?? undefined;
          return [...timings];
        } else {
          // Phase end without start (shouldn't happen, but handle it)
          return [
            ...timings,
            { phase: msg.phase, endTime: now, elapsedMs: msg.elapsed_ms ?? undefined },
          ];
        }
      });
      // Clear current phase only if it's the one that just ended
      if (this.currentPhase() === msg.phase) {
        this.currentPhase.set(null);
      }
      this.outputLog.update((l) => [...l, `[phase] ${msg.phase} ✓ ${msg.elapsed_ms} ms`]);
    }
  }

  private handleRuntimeEvent(event: RuntimeEvent): void {
    switch (event.type) {
      case 'connected':
        this.runtimeConnected.set(true);
        this.outputLog.update((log) => [...log, `[runtime] Connected (${event.mode})`]);
        void this.appVersion.reportServerVersion(event.serverVersion);
        break;
      case 'log':
        this.outputLog.update((log) => [...log, `[${event.level}] ${event.message}`]);
        break;
      case 'phase-start':
        this.handlePhaseMarker({ phase: event.phase, event: 'start' });
        break;
      case 'phase-end':
        this.handlePhaseMarker({
          phase: event.phase,
          event: 'end',
          elapsed_ms: event.elapsedMs ?? null,
        });
        break;
      case 'progress':
        this.outputLog.update((log) => [
          ...log,
          `Progress: ${event.currentLayer} / ${event.totalLayers} layers`,
        ]);
        break;
      case 'slice-complete':
        this.historyVersion.update((v) => v + 1);
        this.currentPhase.set(null);
        break;
      case 'error':
        if (event.error.code === 'not_ready' || event.error.code === 'transport_error') {
          this.runtimeConnected.set(false);
        }
        this.status.set('error');
        this.outputLog.update((log) => [...log, `[error] ${event.error.message}`]);
        break;
    }
  }

  canRetryConnection(): boolean {
    return this.runtimeMode === 'cloud' && this.wsConnection.isFailed();
  }

  retryConnection(): void {
    if (this.runtimeMode === 'cloud') {
      this.wsConnection.retry();
    }
  }

  downloadGcode(): void {
    const url = this.gcodeDownloadUrl();
    if (!url) {
      return;
    }
    void this.downloadFromUrl(url, this.currentGcodeFilename());
  }

  currentGcodeFilename(): string {
    return this.workplateNames.gcodeFilenameFor(
      this.currentRequestUuid(),
      this.slicerFile.sourceFilename() ?? this.selectedFile()?.name,
    );
  }

  selectFile(file: File): void {
    // Selecting a file via the standard input path clears any native selection.
    this.pendingNativeMeshInput = null;
    this.slicerFile.selectFile(file);
    this.status.set('ready');
    this.outputLog.update((log) => [
      ...log,
      `File selected: ${file.name} (${(file.size / 1024 / 1024).toFixed(1)} MB)`,
    ]);
  }

  /** Open a native OS file-picker (Tauri only).
   *  Returns `true` when a file was selected, `false` when cancelled or
   *  when the native picker is unavailable (falls back to `<input type="file">`). */
  async openAndSelectFile(): Promise<boolean> {
    if (!this.runtime.openFilePicker) {
      return false;
    }

    const meshInput = await this.runtime.openFilePicker();
    if (!meshInput) {
      return false;
    }

    this.pendingNativeMeshInput = meshInput;
    // Create a minimal File for the selectedFile signal (filename display).
    // Bytes are intentionally absent for native-picker files; the File has no
    // content but the filename is correct for all UI label consumers.
    const file = new File([], meshInput.fileName);
    this.slicerFile.selectFile(file);
    this.status.set('ready');
    this.outputLog.update((log) => [...log, `File selected: ${meshInput.fileName}`]);
    await this.orchestrator.addMesh(meshInput);
    return true;
  }

  async startWorkplate(file: File): Promise<WorkplateStart> {
    // Clear every trace of the previous plate (file, slice results, scene
    // objects, view mode) before adopting the new model so nothing bleeds
    // across — the old download URL, thumbnail source or a leftover scene
    // object would otherwise ride along into the new workplate.
    await this.resetWorkplate();
    this.selectFile(file);

    if (this.runtimeMode !== 'cloud') {
      const requestUuid = this.createLocalRequestId();
      this.slicerFile.adoptLocal(requestUuid);
      return { requestUuid };
    }

    const uploadMeta = await this.slicerFile.upload();
    return {
      requestUuid: uploadMeta.ruuid,
      uploadMeta,
    };
  }

  updateSettings(patch: Partial<SlicingParams>): void {
    this.settings.update((current) => ({ ...current, ...patch }));
  }

  /**
   * Build the structured slice request: the three active profiles (already in
   * the engine's own shape — no mapping) plus the user's sparse override diff.
   * The diff is every live setting that differs from the resolved profile
   * baseline; the engine re-resolves `profiles → overrides` authoritatively.
   */
  private buildProfileSelection(extraOverrides: Record<string, unknown> = {}): ProfileSelection {
    const baseline = (this.activeSelection.sliceParams() ?? {}) as Record<string, unknown>;
    const settings = this.settings() as unknown as Record<string, unknown>;
    const overrides: Record<string, unknown> = {};
    for (const key of Object.keys(settings)) {
      if (JSON.stringify(settings[key]) !== JSON.stringify(baseline[key])) {
        overrides[key] = settings[key];
      }
    }
    for (const [key, value] of Object.entries(extraOverrides)) {
      overrides[key] = value;
    }
    return {
      printer: this.activeSelection.printer(),
      filament: this.activeSelection.filament(),
      process: this.activeSelection.profile(),
      overrides,
    };
  }

  async slice(): Promise<void> {
    // Guard: prevent concurrent slice operations
    if (
      this.status() !== 'idle' &&
      this.status() !== 'ready' &&
      this.status() !== 'done' &&
      this.status() !== 'error'
    ) {
      console.warn(
        `[Slicer] Cannot slice while ${this.status()}. Wait for current operation to complete.`,
      );
      this.notifications.warning(
        'Slice already in progress',
        'Wait for the current slice to finish',
      );
      return;
    }

    const file = this.selectedFile();
    if (!file) {
      this.notifications.error('No file selected', 'Please upload a model first');
      return;
    }

    // Reset phase state for fresh run
    this.phaseTimings.set([]);
    this.currentPhase.set(null);
    this.lastSliceElapsedMs.set(null);
    this.sliceStartedAt = null;
    this.setDownloadUrl(null);

    // Set up operation abort controller for timeout handling
    this.sliceAbort?.abort();
    this.sliceAbort = new AbortController();

    // Hoisted so the catch block can tell a genuine failure from one caused by
    // this job being superseded (a workplate switch cancels it, nulling
    // `activeSliceId`). Assigned once the job is actually dispatched below.
    let sliceId: string | null = null;

    try {
      const model = await this.readRuntimeMeshInput(file);
      const scene = await this.ensureRuntimeReadyForSlice(model);
      const requestSettings = {
        ...(this.settings() as unknown as Record<string, unknown>),
      };
      const thumbnailOverrides: Record<string, unknown> = {};

      if (this.thumbnailEnabled(requestSettings)) {
        const thumbnail = await this.viewerControl.captureSliceThumbnail({
          sizePx: this.thumbnailSizePx(requestSettings),
          view: this.thumbnailView(requestSettings),
          theme: this.thumbnailTheme(requestSettings),
          colorMode: this.thumbnailColorMode(requestSettings),
          customColor: this.thumbnailCustomColor(requestSettings),
        });
        if (thumbnail) {
          requestSettings['thumbnail_size_px'] = thumbnail.sizePx;
          requestSettings['thumbnail_png_base64'] = thumbnail.pngBase64;
          thumbnailOverrides['thumbnail_size_px'] = thumbnail.sizePx;
          thumbnailOverrides['thumbnail_png_base64'] = thumbnail.pngBase64;
        } else {
          this.outputLog.update((log) => [
            ...log,
            '[warn] Thumbnail capture unavailable — slicing without embedded preview image.',
          ]);
        }
      } else {
        delete requestSettings['thumbnail_png_base64'];
      }
      const profileSelection = this.buildProfileSelection(thumbnailOverrides);

      this.status.set('slicing');
      this.sliceStartedAt = performance.now();
      sliceId = this.createSliceId();
      this.activeSliceId = sliceId;
      this.outputLog.update((log) => [...log, `Starting slice job (${this.runtimeMode})…`]);

      const timeoutHandle = setTimeout(() => {
        if (this.status() === 'slicing') {
          this.status.set('error');
          this.outputLog.update((log) => [
            ...log,
            `[error] Slice operation timed out after ${SLICE_TIMEOUT_MS / 1000 / 60} minutes`,
          ]);
          this.notifications.error(
            'Slice timeout',
            'Operation took too long. Runtime may be overloaded.',
          );
          this.sliceAbort?.abort();
        }
      }, SLICE_TIMEOUT_MS);
      this.sliceAbort.signal.addEventListener('abort', () => clearTimeout(timeoutHandle));

      const result = await this.orchestrator.slice({
        sliceId,
        request_uuid: this.slicerFile.requestUuid() ?? undefined,
        model,
        scene,
        settings: requestSettings,
        profiles: profileSelection,
      });

      // A workplate switch (opening another plate / history entry) cancels the
      // active slice via `clearSliceState`, which nulls `activeSliceId`. If that
      // happened while we were awaiting, this job has been superseded — bail out
      // so its results are never published onto whatever plate is now open.
      if (this.activeSliceId !== sliceId) {
        return;
      }

      const preview = await this.orchestrator.getPreviewSource(result.sliceId);
      if (this.activeSliceId !== sliceId) {
        return;
      }

      // No awaits below this point — publish every result synchronously so a
      // concurrent workplate switch cannot interleave a partial update.
      // Record which objects this slice was produced from so the viewer can
      // preserve its layer/progress/coloring when the same scene is resliced.
      this.slicedObjectIds.set(scene.objects.map((object) => object.id));
      // Snapshot the scene+settings signature so we can detect later drift.
      this.slicedSignature.set(this.sceneSignature());

      if (preview.kind === 'download-url') {
        this.setDownloadUrl(preview.url);
      }
      if (preview.kind === 'gcode-inline') {
        const url = URL.createObjectURL(
          new Blob([preview.gcode], {
            type: 'text/plain;charset=utf-8',
          }),
        );
        this.setDownloadUrl(url);
      }
      if (preview.kind === 'none' && result.downloadUrl) {
        this.setDownloadUrl(result.downloadUrl);
      }

      if (this.sliceStartedAt != null) {
        this.lastSliceElapsedMs.set(Math.round(performance.now() - this.sliceStartedAt));
      }
      this.status.set('done');
      this.currentPhase.set(null);
      this.notifications.success(
        'Slice complete',
        `${result.layerCount} layers — click Download to save G-code`,
        6000,
      );
      this.outputLog.update((log) => [
        ...log,
        `Slice complete — ${result.layerCount} layers generated.`,
      ]);
      this.activeSliceId = null;
    } catch (error) {
      // Swallow the failure of a slice that was superseded by a workplate
      // switch — surfacing an error here would wrongly mark the new plate as
      // failed.
      if (this.activeSliceId !== sliceId) {
        return;
      }
      this.status.set('error');
      const errorMsg = error instanceof Error ? error.message : String(error);
      this.outputLog.update((log) => [...log, `[error] Slice failed: ${errorMsg}`]);
      this.notifications.error('Slice failed', errorMsg);
      this.activeSliceId = null;
    }
  }

  /**
   * Discard the transient results of the current plate — the in-flight job,
   * slice output, progress timings, download URL and the sliced-scene
   * signatures — without touching the selected file's identity. Safe to call
   * repeatedly. The G-code preview clears itself reactively off the workplate
   * uuid, so it is intentionally not touched here (which would also introduce
   * a circular dependency).
   */
  private clearSliceState(): void {
    if (this.activeSliceId) {
      void this.orchestrator.cancel(this.activeSliceId);
      this.activeSliceId = null;
    }
    this.status.set(this.selectedFile() ? 'ready' : 'idle');
    this.outputLog.set([]);
    this.phaseTimings.set([]);
    this.currentPhase.set(null);
    this.sliceStartedAt = null;
    this.lastSliceElapsedMs.set(null);
    this.setDownloadUrl(null);
    this.slicedObjectIds.set([]);
    this.slicedSignature.set(null);
  }

  reset(): void {
    this.pendingNativeMeshInput = null;
    this.slicerFile.reset();
    this.clearSliceState();
    this.viewerControl.viewMode.set('model');
  }

  /**
   * Full teardown for opening a brand-new or empty plate: drops the file and
   * all slice results (via {@link reset}) and empties the scene engine so no
   * geometry from the previous plate survives. Every "start a workplate" and
   * "open an empty plate" entry point funnels through here so the reset is
   * applied uniformly.
   */
  async resetWorkplate(): Promise<void> {
    this.reset();
    await this.sceneEngine.clear();
  }

  getHistory(): Promise<RuntimeHistorySession[]> {
    return this.orchestrator.getHistory();
  }

  /**
   * Drop all persisted slicing history (and the engine's G-code cache, where
   * applicable), then bump {@link historyVersion} so any history view refetches
   * the now-empty list. No-op on the web/wasm runtime.
   */
  async clearHistory(): Promise<void> {
    await this.orchestrator.clearHistory();
    this.historyVersion.update((v) => v + 1);
  }

  async downloadHistorySession(session: RuntimeHistorySession): Promise<void> {
    const filename = this.workplateNames.gcodeFilenameFor(
      session.request_uuid,
      session.original_filename,
    );

    if (!session.download_url) {
      const preview = await this.orchestrator.getPreviewSource(session.request_uuid);
      if (preview.kind === 'download-url') {
        await this.downloadFromUrl(preview.url, filename);
        return;
      }
      if (preview.kind === 'gcode-inline') {
        const url = URL.createObjectURL(
          new Blob([preview.gcode], {
            type: 'text/plain;charset=utf-8',
          }),
        );
        await this.downloadFromUrl(url, filename);
        URL.revokeObjectURL(url);
        return;
      }
      this.notifications.warning(
        'No downloadable output',
        'No preview source available for this session',
      );
      return;
    }

    await this.downloadFromUrl(session.download_url, filename);
  }

  private async downloadFromUrl(url: string, filename: string): Promise<void> {
    if (this.runtimeMode === 'native') {
      // Tauri's webview does not reliably honour `<a download>` against a
      // custom `asset://` URL (it's meant for `<img>`/`<video>` src, not
      // forcing a save) — depending on OS/WebView the link just navigates or
      // is silently ignored instead of prompting a save. Use the native
      // "Save As" dialog + filesystem write instead: fetch the bytes (works
      // for both `asset://` URLs and blob URLs) and write them to the
      // user-chosen path.
      try {
        const destination = await save({
          defaultPath: filename,
          filters: [{ name: 'G-code', extensions: ['gcode', 'gco', 'g'] }],
        });
        if (!destination) {
          return; // user cancelled the dialog
        }
        const bytes = new Uint8Array(await (await fetch(url)).arrayBuffer());
        await writeFile(destination, bytes);
        this.notifications.success('Saved', `G-code saved to ${destination}`);
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        this.notifications.error('Save failed', message);
      }
      return;
    }

    const link = document.createElement('a');
    link.href = url;
    link.download = filename;
    link.click();
  }

  private resolveRuntimeMode(): RuntimeMode {
    if (this.isTauriDetected()) {
      return 'native';
    }

    return environment.runtimeMode;
  }

  private isTauriDetected(): boolean {
    const globals = globalThis as unknown as {
      __TAURI__?: unknown;
      __TAURI_INTERNALS__?: unknown;
      navigator?: { userAgent?: string };
    };
    if (globals.__TAURI__ || globals.__TAURI_INTERNALS__) {
      return true;
    }
    return Boolean(globals.navigator?.userAgent?.includes('Tauri'));
  }

  private createSliceId(): string {
    if (globalThis.crypto?.randomUUID) {
      return globalThis.crypto.randomUUID();
    }
    return `slice-${Date.now()}`;
  }

  private createLocalRequestId(): string {
    return `local-${this.createSliceId()}`;
  }

  private async ensureRuntimeReadyForSlice(model: RuntimeMeshInput): Promise<RuntimeSceneSnapshot> {
    if (this.runtimeMode === 'cloud') {
      const requestUuid = this.slicerFile.requestUuid();
      const fileIds = this.slicerFile.fileIds();
      if (!requestUuid || fileIds.length === 0) {
        this.status.set('uploading');
        this.outputLog.update((log) => [...log, 'Uploading file…']);
        await this.orchestrator.addMesh(model);
        this.outputLog.update((log) => [...log, 'Upload complete. Starting slice job…']);
      }
    }

    let scene = this.visibleSceneSnapshot();
    if (
      (this.runtimeMode === 'web' || this.runtimeMode === 'native') &&
      scene.objects.length === 0
    ) {
      await this.orchestrator.addMesh(model);
      scene = this.visibleSceneSnapshot();
    }

    return scene;
  }

  private async readRuntimeMeshInput(file: File): Promise<RuntimeMeshInput> {
    // When the user picked a file through the native OS dialog, return the
    // pre-built input directly. It already has `filePath` set so the Tauri
    // runtime will pass only the path to Rust, skipping `arrayBuffer()` here
    // and avoiding large byte arrays crossing the IPC channel during slicing.
    if (this.pendingNativeMeshInput) {
      return this.pendingNativeMeshInput;
    }
    return {
      fileName: file.name,
      format: this.fileFormatFromName(file.name),
      bytes: new Uint8Array(await file.arrayBuffer()),
    };
  }

  private visibleSceneSnapshot(): RuntimeSceneSnapshot {
    const snapshot = this.sceneEngine.snapshot();
    return {
      objects: snapshot.objects.map((object) => ({
        id: object.id.toString(),
        name: object.name,
        translation: object.translation,
        euler_xyz_deg: object.euler_xyz_deg,
        scale: object.scale,
        triangle_count: object.triangle_count,
        world_aabb: object.world_aabb,
      })),
    };
  }

  private fileFormatFromName(fileName: string): 'stl' | 'obj' | '3mf' {
    const lower = fileName.toLowerCase();
    if (lower.endsWith('.obj')) {
      return 'obj';
    }
    if (lower.endsWith('.3mf')) {
      return '3mf';
    }
    return 'stl';
  }

  private thumbnailEnabled(settings: Record<string, unknown>): boolean {
    const raw = settings['thumbnail_enabled'];
    if (raw === undefined || raw === null) {
      return true;
    }
    return raw === true || raw === 'true' || raw === 1;
  }

  private thumbnailSizePx(settings: Record<string, unknown>): number {
    const raw = Number(settings['thumbnail_size_px']);
    if (!Number.isFinite(raw)) {
      return DEFAULT_THUMBNAIL_SIZE_PX;
    }
    return Math.max(64, Math.min(1024, Math.round(raw)));
  }

  private thumbnailView(settings: Record<string, unknown>): ThumbnailView {
    const raw = settings['thumbnail_view'];
    switch (raw) {
      case 'front':
      case 'rear':
      case 'left':
      case 'right':
      case 'top':
      case 'isometric':
        return raw;
      default:
        return 'isometric';
    }
  }

  private thumbnailTheme(settings: Record<string, unknown>): ThumbnailTheme {
    const raw = settings['thumbnail_theme'];
    return raw === 'dark' || raw === 'transparent' ? raw : 'light';
  }

  private thumbnailColorMode(settings: Record<string, unknown>): ThumbnailColorMode {
    const raw = settings['thumbnail_color_mode'];
    return raw === 'generic' || raw === 'custom' ? raw : 'filament';
  }

  private thumbnailCustomColor(settings: Record<string, unknown>): string {
    const raw = settings['thumbnail_custom_color'];
    return typeof raw === 'string' && /^#[0-9a-fA-F]{6}$/.test(raw) ? raw : '#e0912f';
  }

  private setDownloadUrl(url: string | null): void {
    if (this.objectUrl) {
      URL.revokeObjectURL(this.objectUrl);
      this.objectUrl = null;
    }

    if (url?.startsWith('blob:')) {
      this.objectUrl = url;
    }

    this.gcodeDownloadUrl.set(url);
  }
}
