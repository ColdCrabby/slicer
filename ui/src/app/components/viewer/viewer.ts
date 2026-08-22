import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  computed,
  effect,
  inject,
  input,
  output,
  signal,
  untracked,
  viewChild,
} from '@angular/core';
import { BufferAttribute, BufferGeometry, Matrix4, Mesh, MeshPhongMaterial } from 'three';
import { AppTheme } from '../../services/app-theme';
import { GcodePreview, ROLE_LABELS, scalarChannelFor } from '../../services/gcode-preview';
import { ObjectTracker } from '../../services/object-tracker';
import { PrintArea } from '../../services/print-area';
import { SceneCommand } from '../../services/scene-command/scene-command';
import { SceneEngine } from '../../services/scene-engine';
import { ViewerControl } from '../../services/viewer-control';
import { GcodeHoverProbe, type GcodeHoverHit } from './gcode-hover';
import { GcodeOrchestrator } from './gcode-orchestrator';
import type { GizmoDelta } from './gizmo';
import { ViewerScene } from './scene';

export type ViewerMode = 'model' | 'gcode';

/** Input accepted by the model input. */
export type ModelSource = string | URL | File | Blob | ArrayBuffer;

/**
 * Single-component 3D viewer for both raw meshes and sliced G-code.
 *
 * The viewer is the only entry point for visualization. It owns the
 * Three.js scene, switches between the two render modes without
 * re-initializing WebGL, and drives the streaming G-code pipeline.
 *
 * Usage:
 * ```html
 * <nexus-viewer [model]="stlFileOrUrl" mode="model"></nexus-viewer>
 * <nexus-viewer mode="gcode"></nexus-viewer>
 * ```
 */
@Component({
  selector: 'nexus-viewer',
  standalone: true,
  templateUrl: './viewer.html',
  styleUrl: './viewer.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class Viewer {
  readonly mode = input<ViewerMode>('model');
  readonly model = input<ModelSource | null>(null);
  readonly showTravel = input(false);

  readonly loadComplete = output<{ mode: ViewerMode; segments: number }>();
  readonly loadError = output<{ mode: ViewerMode; error: unknown }>();

  private readonly hostRef = viewChild.required<ElementRef<HTMLElement>>('host');
  private readonly elementRef = inject(ElementRef);
  private readonly viewerControl = inject(ViewerControl);
  private readonly printArea = inject(PrintArea);
  private readonly objectTracker = inject(ObjectTracker);
  private readonly sceneEngine = inject(SceneEngine);
  private readonly sceneCommand = inject(SceneCommand);
  private readonly gcodePreview = inject(GcodePreview);
  private readonly appTheme = inject(AppTheme);
  private readonly destroyRef = inject(DestroyRef);

  /** Current loading status for the optional overlay. */
  readonly status = signal<'idle' | 'loading' | 'streaming' | 'ready' | 'error'>('idle');
  /** Smoothed frames-per-second reported by the render loop. */
  readonly fps = signal(0);
  /** Smoothed average frame delay in milliseconds. */
  readonly frameDelayMs = signal(0);
  /**
   * End-to-end wall time of the last WASM mesh round-trip, measured from
   * the moment the bytes are handed to `addMesh` to the moment the
   * `RenderBuffer` is fully copied out of WASM memory. `null` until the
   * first model has been loaded through the engine.
   */
  readonly wasmRoundtripMs = signal<number | null>(null);
  /** WASM-side parse time (`addMesh`) of the last model load. */
  readonly wasmParseMs = signal<number | null>(null);
  /** WASM-side render-buffer extraction time (`getRenderBuffer`) of the last load. */
  readonly wasmRenderBufMs = signal<number | null>(null);
  /** Last-op rolling stats from the scene engine, surfaced in the overlay. */
  readonly opStats = computed(() => this.sceneEngine.opStats());
  private readonly progressSegments = signal(0);
  private readonly errorMessage = signal<string>('');

  /** Inspector readout for the extrusion under the cursor (G-code scalar views). */
  readonly hoverInfo = this.gcodePreview.hoverInfo;
  /** Role display labels for the hover tooltip. */
  protected readonly roleLabels = ROLE_LABELS;

  private scene: ViewerScene | null = null;
  private gcode: GcodeOrchestrator | null = null;
  private gcodeHover: GcodeHoverProbe | null = null;
  private currentAbort: AbortController | null = null;
  private loadToken = 0;
  /** SceneObject ids registered for the currently-loaded source. */
  private trackedObjectIds: string[] = [];
  /** Live mapping from WASM scene id to the Three.js mesh that mirrors it. */
  private readonly wasmMeshes = new Map<bigint, Mesh>();
  /**
   * The model source that is currently loaded into the WASM scene engine.
   * Used to detect whether a source change is a new model (full teardown)
   * or just a mode switch (WASM state preserved, transforms kept).
   */
  private activeModelSource: ModelSource | null = null;
  private readonly tmpMatrix = new Matrix4();
  /**
   * Currently selected WASM object ids (as bigint), kept in sync with the
   * legacy scene's string-id selection set so highlight + drag work.
   */
  private selectedWasmIds: bigint[] = [];
  /**
   * Per-axis displacement (mm) already pushed to the engine for the
   * in-flight drag, indexed by WASM id. (Currently unused — the gizmo
   * already reports per-frame deltas — retained as a stub in case a
   * cumulative-protocol drag handler is reintroduced later.)
   */
  private dragApplied = new Map<bigint, { dx: number; dy: number }>();

  constructor() {
    afterNextRender(() => this.initScene());

    this.destroyRef.onDestroy(() => {
      this.cancelInFlightLoad();
      this.gcodeHover?.dispose();
      this.gcodeHover = null;
      this.gcode?.dispose();
      this.gcode = null;
      this.scene?.dispose();
      this.scene = null;
      this.viewerControl.orbitSink = null;
    });

    // React to input changes — single effect handles mode + source switching.
    effect(() => {
      const mode = this.mode();
      const model = this.model();

      if (!this.scene) {
        return;
      }
      this.applySource(mode, model);
    });

    // React to view-preset changes from the toolbar.
    effect(() => {
      const view = this.viewerControl.view();
      this.scene?.setView(view);
    });

    // React to object-mode (gizmo) changes from the toolbar.
    effect(() => {
      const mode = this.viewerControl.objectMode();
      this.scene?.setObjectMode(mode);
    });

    // React to reset requests from the toolbar.
    effect(() => {
      const tick = this.viewerControl.resetTick();
      // Skip the very first emission so we don't redundantly reset on init.
      if (tick === 0) {
        return;
      }
      this.scene?.resetView();
    });

    // React to direction-look requests (e.g. from the viewport-cube gizmo).
    effect(() => {
      const req = this.viewerControl.lookRequest();
      if (!req) {
        return;
      }
      this.scene?.animateToDirection(req.direction, req.up);
    });

    // Mirror the print-area configuration into the scene so the bed grid
    // tracks any settings/UI changes (dimensions or movable-area offset).
    effect(() => {
      const config = this.printArea.config();
      this.scene?.setPrintArea(config);
    });

    // Mirror the application's selection state into the scene so meshes get
    // their highlight as soon as the service signal flips (whether the flip
    // came from a viewer click or from external UI).
    effect(() => {
      const ids = this.printArea.selectedIds();
      this.scene?.setSelectedIds(ids);
    });

    // Mirror tracked-object transforms onto the corresponding mesh nodes.
    // Reading every SceneObject's `transform` signal in this effect makes
    // it depend on each object's position/rotation/scale, so any update
    // (manual API call, drag, future gizmo) re-runs and pushes through.
    //
    // DISABLED during migration to the WASM scene engine. Object transforms
    // now flow through `wasmMeshes` (see effect below); the legacy tracker
    // path is kept dormant for the eventual selection / gizmo work.
    // effect(() => {
    //   const objects = this.objectTracker.objects();
    //   const scene = this.scene;
    //   if (!scene) {
    //     return;
    //   }
    //   for (const obj of objects) {
    //     scene.setObjectTransform(obj.id, obj.transform());
    //   }
    // });

    // Push the WASM scene-engine transform onto each mirrored Three.js mesh
    // every time the snapshot changes. This is the equivalent of the legacy
    // ObjectTracker mirror above — only the source of truth has moved into
    // Rust. Matrices are read column-major (matches glam) and applied with
    // `matrixAutoUpdate = false` so Three.js does not overwrite them.
    effect(() => {
      const objects = this.sceneEngine.objects();
      if (this.wasmMeshes.size === 0) {
        return;
      }
      for (const obj of objects) {
        const mesh = this.wasmMeshes.get(obj.id);
        if (!mesh) {
          continue;
        }
        const m = this.sceneEngine.getMatrix(obj.id);
        this.tmpMatrix.fromArray(m);
        mesh.matrix.copy(this.tmpMatrix);
        mesh.matrixWorldNeedsUpdate = true;
      }
    });

    // React to layer-range changes from the GcodePreviewService.
    effect(() => {
      const min = this.gcodePreview.layerMin();
      const max = this.gcodePreview.layerMax();
      this.gcode?.showRange(min, max);
    });

    // React to nozzle-progress changes.
    effect(() => {
      const progress = this.gcodePreview.segmentProgress();
      const max = this.gcodePreview.layerMax();
      this.gcode?.applyProgress(max, progress);
    });

    // React to role visibility changes.
    effect(() => {
      const hidden = this.gcodePreview.hiddenRoles();
      this.gcode?.applyHiddenRoles(hidden);
    });

    // React to theme, view-mode, scalar-range, fan-selection, or legend
    // hover-band changes — recolor all layers in place without rebuilding.
    effect(() => {
      const mode = this.gcodePreview.viewMode();
      const colors = this.gcodePreview.roleColors();
      const range = this.gcodePreview.activeRange();
      const fan = this.gcodePreview.selectedFan();
      const band = this.gcodePreview.hoverBand();
      this.gcode?.applyView(colors, scalarChannelFor(mode), range, fan, band);
    });

    // The hover-inspect probe is only meaningful in the G-code scalar views.
    effect(() => {
      const active =
        this.mode() === 'gcode' && scalarChannelFor(this.gcodePreview.viewMode()) !== null;
      this.gcodeHover?.setEnabled(active);
      if (!active) {
        this.gcodePreview.setHoverInfo(null);
      }
    });

    // Build (or rebuild) the layer graph when the parsed handle becomes
    // available or is replaced. This is intentionally separate from the
    // `applySource` effect so that layer-range / role / progress slider ticks
    // (which `applySource` reads via `untracked`) do not redundantly tear
    // down and rebuild every layer group.
    effect(() => {
      const handle = this.gcodePreview.gcodeHandle();
      if (!handle || untracked(() => this.mode()) !== 'gcode' || !this.scene) {
        return;
      }
      this.startGcodeFromHandle();
    });
  }

  // ---------------------------------------------------------------------------
  // Selection / gizmo handlers
  //
  // The legacy `ViewerScene` raycast pointer plumbing calls `handleSelect` /
  // `handleClearSelection`; gizmo-driven object manipulation goes through
  // `handleGizmoDelta` / `handleGizmoEnd` / `handleFacePicked` which
  // dispatch one WASM op per selected object id.
  // ---------------------------------------------------------------------------

  /**
   * Map a hover hit to the active channel value and publish it for the
   * inspector tooltip + legend tick, or clear the readout when nothing
   * extrudable is under the cursor.
   */
  private onGcodeHover(hit: GcodeHoverHit | null): void {
    if (!hit) {
      this.gcodePreview.setHoverInfo(null);
      return;
    }
    const mode = this.gcodePreview.viewMode();
    const channel = scalarChannelFor(mode);
    if (!channel) {
      this.gcodePreview.setHoverInfo(null);
      return;
    }

    const rs = hit.ref.roleSegments;
    const i = hit.instanceId;
    const width = rs.widths?.[i] ?? 0;
    const height = rs.heights?.[i] ?? 0;
    const speed = rs.speeds?.[i] ?? 0;
    const value =
      channel.scope === 'segment'
        ? channel.extract(width, height, speed)
        : channel.extractLayer(hit.ref.meta, this.gcodePreview.selectedFan());
    if (value === null) {
      this.gcodePreview.setHoverInfo(null);
      return;
    }

    const range = this.gcodePreview.activeRange();
    const span = range.max - range.min;
    const t = span > 0 ? Math.min(1, Math.max(0, (value - range.min) / span)) : 0.5;
    const rect = this.elementRef.nativeElement.getBoundingClientRect();
    this.gcodePreview.setHoverInfo({
      channelId: mode,
      value,
      valueLabel: channel.format(value),
      role: rs.role,
      layerIndex: hit.ref.layerIndex,
      z: hit.ref.z,
      width,
      height,
      speed,
      t,
      x: hit.clientX - rect.left,
      y: hit.clientY - rect.top,
    });
  }

  private handleSelect(stringId: string, _additive: boolean): void {
    const id = parseWasmId(stringId);
    if (id === null) {
      return;
    }
    // Multi-select with click-to-toggle: clicking an unselected object
    // adds it to the selection; clicking an already-selected object
    // removes it. No modifier keys required.
    const idx = this.selectedWasmIds.indexOf(id);
    if (idx === -1) {
      this.selectedWasmIds = [...this.selectedWasmIds, id];
    } else {
      this.selectedWasmIds = this.selectedWasmIds.filter((existing) => existing !== id);
    }
    this.scene?.setSelectedIds(new Set(this.selectedWasmIds.map(String)));
  }

  private handleClearSelection(): void {
    this.selectedWasmIds = [];
    this.scene?.setSelectedIds(new Set());
  }

  /** Translate / rotate / scale a delta onto every currently-selected object. */
  private handleGizmoDelta(stringIds: readonly string[], delta: GizmoDelta): void {
    for (const stringId of stringIds) {
      const id = parseWasmId(stringId);
      if (id === null) {
        continue;
      }
      switch (delta.kind) {
        case 'translate':
          this.sceneCommand.apply({
            op: 'Translate',
            args: { id, delta: delta.delta },
          });
          break;
        case 'rotate':
          this.sceneCommand.apply({
            op: 'Rotate',
            args: { id, axis: delta.axis, degrees: delta.degrees },
          });
          break;
        case 'scale':
          this.sceneCommand.apply({
            op: 'Scale',
            args: { id, factors: delta.factors },
          });
          break;
      }
    }
  }

  /** Flush any in-progress gesture so the history entry is committed. */
  private handleGizmoEnd(): void {
    // When gravity is enabled, drop every selected object to the floor before
    // committing the history entry. This keeps the drop part of the same
    // gesture so undo reverts the entire move + drop as one unit.
    if (this.viewerControl.gravityEnabled()) {
      for (const id of this.selectedWasmIds) {
        this.sceneCommand.apply({ op: 'DropToFloor', args: { id } });
      }
    }
    this.sceneCommand.flush();
  }

  /**
   * Pull-to-floor: align the picked face to Z=0. Stays in pull-to-floor
   * mode so the user can pick another face on another object without
   * having to re-enter the mode. Selection is left untouched — picking a
   * face is a manipulation gesture, not a selection gesture.
   */
  private handleFacePicked(stringId: string, faceIndex: number): void {
    const id = parseWasmId(stringId);
    if (id === null) {
      return;
    }
    this.sceneCommand.apply({
      op: 'PlaceFaceOnFloor',
      args: { id, face_index: faceIndex },
    });
    this.sceneCommand.flush();
  }

  statusLabel(): string {
    switch (this.status()) {
      case 'loading':
        return 'Loading…';
      case 'streaming':
        return `Streaming… ${this.progressSegments().toLocaleString()} segments`;
      case 'ready':
        return `Ready — ${this.progressSegments().toLocaleString()} segments`;
      case 'error': {
        const detail = this.errorMessage();
        return detail ? `Failed to load — ${detail}` : 'Failed to load';
      }
      default:
        return '';
    }
  }

  private initScene(): void {
    const host = this.hostRef().nativeElement;
    this.scene = new ViewerScene(host, this.printArea.config());
    // Mirror the live camera direction/up into ViewerControl so external
    // overlays (the viewport-cube gizmo) can read it without going through
    // Angular's change-detection.
    const state = this.viewerControl.cameraState;
    this.scene.cameraStateSink = (dir, up, fov) => {
      state.direction.copy(dir);
      state.up.copy(up);
      state.fov = fov;
    };
    this.scene.fpsSink = (fps, delayMs) => {
      this.fps.set(fps);
      this.frameDelayMs.set(delayMs);
    };
    // Allow external gizmos (viewport-cube drag) to orbit the main camera.
    this.viewerControl.orbitSink = (azimuth, polar) => this.scene?.orbitBy(azimuth, polar);
    // Bridge raycast hits / gizmo gestures from the scene into the WASM
    // scene engine. Selection is stored locally; object manipulation is
    // driven by the gizmo (translate / rotate / scale) and pull-to-floor.
    this.scene.selectionHandlers = {
      select: (id, additive) => this.handleSelect(id, additive),
      clearSelection: () => this.handleClearSelection(),
    };
    this.scene.gizmoHandlers = {
      delta: (ids, delta) => this.handleGizmoDelta(ids, delta),
      end: () => this.handleGizmoEnd(),
      facePicked: (objectId, faceIndex) => this.handleFacePicked(objectId, faceIndex),
    };
    // Apply the current toolbar selections so the scene starts in sync with
    // whatever view / object mode the user already had selected.
    this.scene.setObjectMode(this.viewerControl.objectMode());
    this.scene.setView(this.viewerControl.view());
    this.gcode = new GcodeOrchestrator(this.scene.contentRoot);
    // Hover-inspect probe for the G-code scalar views: raycasts the visible
    // layer meshes and reports the extrusion value under the cursor.
    this.gcodeHover = new GcodeHoverProbe(
      this.scene.renderer.domElement,
      this.scene.camera,
      () => this.gcode?.hoverableMeshes() ?? [],
      (hit) => this.onGcodeHover(hit),
    );
    this.gcodeHover.setEnabled(
      this.mode() === 'gcode' && scalarChannelFor(this.gcodePreview.viewMode()) !== null,
    );
    // Seed the bed grid from the current print-area configuration.
    this.scene.setPrintArea(this.printArea.config());
    // Trigger initial source application now that the scene exists.
    this.applySource(this.mode(), this.model());
  }

  private applySource(mode: ViewerMode, model: ModelSource | null): void {
    const scene = this.scene;
    if (!scene) {
      return;
    }
    this.cancelInFlightLoad();

    const modelChanged = model !== this.activeModelSource;

    // Always tear down the G-code orchestrator and clear Three.js content —
    // the display layer is rebuilt for every mode/source transition.
    this.gcode?.dispose();
    scene.clearContent();
    this.progressSegments.set(0);
    this.errorMessage.set('');

    if (modelChanged) {
      // New model source — full teardown of WASM engine objects so ids do
      // not accumulate and the old mesh's transforms are discarded cleanly.
      for (const id of this.trackedObjectIds) {
        this.printArea.forgetObject(id);
        this.objectTracker.remove(id);
      }
      this.trackedObjectIds = [];
      for (const id of this.wasmMeshes.keys()) {
        this.scene?.unregisterSelectable(String(id));
        try {
          this.sceneEngine.apply({ op: 'Remove', args: { id } });
        } catch {
          // Object may already be gone if the engine reset; safe to ignore.
        }
      }
      this.wasmMeshes.clear();
      this.handleClearSelection();
      this.dragApplied.clear();
      this.activeModelSource = model;
    } else {
      // Mode switch only (e.g. model → gcode → model). The WASM scene engine
      // still holds the object with its current transforms intact. Unregister
      // the now-disposed Three.js selectables so raycasts do not hit stale
      // nodes, but do NOT issue Remove ops — that would wipe the transforms.
      for (const id of this.wasmMeshes.keys()) {
        this.scene?.unregisterSelectable(String(id));
      }
      this.wasmMeshes.clear();
      this.handleClearSelection();
    }

    if (mode === 'model') {
      if (!model) {
        this.status.set('idle');
        return;
      }
      // If the WASM engine already holds objects for this source (mode switch,
      // not a new file), re-render directly from engine state so transforms
      // are preserved without a second parse round-trip.
      const existingObjects = untracked(() => this.sceneEngine.objects());
      if (!modelChanged && existingObjects.length > 0) {
        void this.rebuildThreeJsMeshes();
      } else {
        this.startModelLoad(model);
      }
    } else {
      // G-code is rendered exclusively through the WASM GcodeHandle path,
      // which gives per-layer / per-role geometry. The old TS streaming
      // fallback (ChunkedLineGeometry / startGcodeLoad) is intentionally
      // not used: it produced a flat cyan mesh and was only ever a
      // temporary stand-in before the WASM parser existed.
      // Read gcodeHandle untracked: layer/role/progress changes flow through
      // their own dedicated effects, and we must not rebuild the whole layer
      // graph (and re-fit the camera) on every slider tick.
      if (untracked(() => this.gcodePreview.gcodeHandle())) {
        this.startGcodeFromHandle();
      } else {
        // Handle not yet available — either loading is in progress or no
        // source has been dispatched yet. Hold at loading/idle and let the
        // gcodeHandle effect below call startGcodeFromHandle once ready.
        this.status.set(untracked(() => this.gcodePreview.loading()) ? 'loading' : 'idle');
      }
    }
  }

  /**
   * Re-render Three.js display meshes from objects already held by the WASM
   * scene engine. Called when switching back to model view after a mode switch
   * (e.g. model → gcode → model) so that user-applied transforms are not lost.
   */
  private async rebuildThreeJsMeshes(): Promise<void> {
    await this.sceneEngine.ready();
    if (!this.scene) {
      return;
    }
    const objects = untracked(() => this.sceneEngine.objects());
    for (const obj of objects) {
      const buf = this.sceneEngine.getRenderBuffer(obj.id);
      const geometry = new BufferGeometry();
      geometry.setAttribute('position', new BufferAttribute(buf.positions, 3));
      geometry.setAttribute('normal', new BufferAttribute(buf.normals, 3));
      geometry.setIndex(new BufferAttribute(buf.indices, 1));
      geometry.computeBoundingBox();
      geometry.computeBoundingSphere();
      const material = new MeshPhongMaterial({
        color: 0xa9b4c2,
        flatShading: true,
        shininess: 16,
      });
      const mesh = new Mesh(geometry, material);
      mesh.name = obj.name;
      mesh.matrixAutoUpdate = false;
      this.tmpMatrix.fromArray(this.sceneEngine.getMatrix(obj.id));
      mesh.matrix.copy(this.tmpMatrix);
      mesh.matrixWorldNeedsUpdate = true;
      this.scene.contentRoot.add(mesh);
      this.wasmMeshes.set(obj.id, mesh);
      mesh.userData['faceGroups'] = this.sceneEngine.getFaceGroups(obj.id);
      this.scene.registerSelectable(String(obj.id), mesh);
    }
    this.status.set('ready');
    this.loadComplete.emit({ mode: 'model', segments: 0 });
  }

  private startModelLoad(source: ModelSource): void {
    const scene = this.scene;
    if (!scene) {
      return;
    }
    const token = ++this.loadToken;
    this.status.set('loading');

    // New WASM-scene-engine path: fetch raw bytes, parse them inside
    // Rust, then build a BufferGeometry from the WASM-emitted render
    // buffer. The scene-engine owns the mesh data and the transform; the
    // Three.js node is a thin display mirror with `matrixAutoUpdate = false`.
    void this.loadModelViaSceneEngine(source, token).catch((error: unknown) => {
      if (token !== this.loadToken) {
        return;
      }
      this.errorMessage.set(messageOf(error));
      this.status.set('error');
      this.loadError.emit({ mode: 'model', error });
    });
  }

  private async loadModelViaSceneEngine(source: ModelSource, token: number): Promise<void> {
    await this.sceneEngine.ready();
    if (token !== this.loadToken || !this.scene) {
      return;
    }
    const { bytes, format, name } = await readModelBytes(source);
    if (token !== this.loadToken || !this.scene) {
      return;
    }
    // Time each phase of the WASM round-trip independently so the overlay
    // can break down where wall time is spent (parse vs. render-buffer
    // copy). `performance.now()` returns a high-resolution monotonic clock.
    const tParseStart = performance.now();
    const id = this.sceneEngine.addMesh(name, format, bytes);
    const tParseEnd = performance.now();
    const buf = this.sceneEngine.getRenderBuffer(id);
    const tRenderBufEnd = performance.now();
    this.wasmParseMs.set(tParseEnd - tParseStart);
    this.wasmRenderBufMs.set(tRenderBufEnd - tParseEnd);
    this.wasmRoundtripMs.set(tRenderBufEnd - tParseStart);
    const geometry = new BufferGeometry();
    geometry.setAttribute('position', new BufferAttribute(buf.positions, 3));
    geometry.setAttribute('normal', new BufferAttribute(buf.normals, 3));
    geometry.setIndex(new BufferAttribute(buf.indices, 1));
    geometry.computeBoundingBox();
    geometry.computeBoundingSphere();
    const material = new MeshPhongMaterial({
      color: 0xa9b4c2,
      flatShading: true,
      shininess: 16,
    });
    const mesh = new Mesh(geometry, material);
    mesh.name = name;
    mesh.matrixAutoUpdate = false;
    // Seed initial matrix so first frame renders correctly even before any
    // op fires the snapshot effect.
    this.tmpMatrix.fromArray(this.sceneEngine.getMatrix(id));
    mesh.matrix.copy(this.tmpMatrix);
    mesh.matrixWorldNeedsUpdate = true;
    this.scene.contentRoot.add(mesh);
    this.wasmMeshes.set(id, mesh);
    // Precompute coplanar face groups and store in userData so the
    // pull-to-floor highlight can light up whole flat regions rather than
    // individual triangles. Groups are computed once here in WASM (O(F) with
    // union-find) and read O(1) per hover frame afterwards.
    mesh.userData['faceGroups'] = this.sceneEngine.getFaceGroups(id);
    // Stamp the same id (stringified) on the legacy scene's selectable
    // registry so the existing raycast / drag pointer plumbing recognises
    // it. The drag handlers translate it back to a bigint.
    this.scene.registerSelectable(String(id), mesh);
    // Auto-orient and drop to bed on first load. Applied directly through
    // the engine (not sceneCommand) so the oriented position is the baseline
    // state and Ctrl+Z does not revert back to the un-oriented pose.
    this.sceneEngine.apply({ op: 'AutoOrient', args: { id } });
    this.sceneEngine.apply({ op: 'DropToFloor', args: { id } });
    // Sync the Three.js mesh matrix to the post-orient transform so the
    // first rendered frame reflects the correct orientation.
    this.tmpMatrix.fromArray(this.sceneEngine.getMatrix(id));
    mesh.matrix.copy(this.tmpMatrix);
    mesh.matrixWorldNeedsUpdate = true;
    this.status.set('ready');
    this.loadComplete.emit({ mode: 'model', segments: 0 });
  }

  /**
   * Render gcode using the parsed `GcodeHandle` from `GcodePreviewService`.
   * Delegates geometry construction to `GcodeOrchestrator`; Three.js only
   * manages layer/segment visibility after this point.
   */
  private startGcodeFromHandle(): void {
    const scene = this.scene;
    const gcode = this.gcode;
    // All gcode-preview reads here must be untracked. This method runs from
    // the `applySource` effect; if any of the layer/role/progress signals
    // were tracked here, every slider tick would re-enter this path and
    // rebuild every layer group + re-fit the camera. The dedicated effects
    // (showRange / applyProgress / applyHiddenRoles) are the sole reactive
    // consumers of those signals.
    const handle = untracked(() => this.gcodePreview.gcodeHandle());
    if (!scene || !gcode || !handle) {
      return;
    }

    this.cancelInFlightLoad();
    const colors = untracked(() => this.gcodePreview.roleColors());
    const { totalSegments } = gcode.buildFromHandle(handle, colors);

    const min = untracked(() => this.gcodePreview.layerMin());
    const max = untracked(() => this.gcodePreview.layerMax());
    const progress = untracked(() => this.gcodePreview.segmentProgress());
    const hidden = untracked(() => this.gcodePreview.hiddenRoles());
    const mode = untracked(() => this.gcodePreview.viewMode());
    const range = untracked(() => this.gcodePreview.activeRange());
    const fan = untracked(() => this.gcodePreview.selectedFan());
    const band = untracked(() => this.gcodePreview.hoverBand());
    gcode.applyView(colors, scalarChannelFor(mode), range, fan, band);
    gcode.showRange(min, max);
    gcode.applyProgress(max, progress);
    gcode.applyHiddenRoles(hidden);
    this.status.set('ready');
    this.loadComplete.emit({ mode: 'gcode', segments: totalSegments });
  }

  private cancelInFlightLoad(): void {
    this.loadToken++;
    if (this.currentAbort) {
      this.currentAbort.abort();
      this.currentAbort = null;
    }
  }
}

function isAbortError(error: unknown): boolean {
  return (
    typeof error === 'object' &&
    error !== null &&
    'name' in error &&
    (error as { name: string }).name === 'AbortError'
  );
}

function messageOf(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === 'string') {
    return error;
  }
  return '';
}

/**
 * Coerce a {@link ModelSource} into the `{ bytes, format, name }` triple
 * expected by `SceneEngineService.addMesh`. The format is detected from the
 * source's filename / URL extension; raw `ArrayBuffer`/`Blob` inputs without
 * a name fall back to STL (the most common payload).
 */
async function readModelBytes(
  source: ModelSource,
): Promise<{ bytes: Uint8Array; format: 'stl' | 'obj' | '3mf'; name: string }> {
  let buffer: ArrayBuffer;
  let name = 'model';
  if (typeof source === 'string') {
    name = source.split(/[\\/]/).pop() ?? 'model';
    const res = await fetch(source);
    buffer = await res.arrayBuffer();
  } else if (source instanceof URL) {
    name = source.pathname.split('/').pop() ?? 'model';
    const res = await fetch(source);
    buffer = await res.arrayBuffer();
  } else if (source instanceof File) {
    name = source.name;
    buffer = await source.arrayBuffer();
  } else if (source instanceof Blob) {
    buffer = await source.arrayBuffer();
  } else {
    buffer = source;
  }
  const ext = name.split('.').pop()?.toLowerCase();
  const format: 'stl' | 'obj' | '3mf' =
    ext === 'obj' || ext === '3mf' || ext === 'stl' ? ext : 'stl';
  return { bytes: new Uint8Array(buffer), format, name };
}

/**
 * Parse the string id stored on the legacy scene's selectable registry back
 * into the WASM `bigint` id used by the scene engine. Returns `null` if the
 * string isn't a valid integer (defensive — should never happen since we
 * stamp the id ourselves at registration time).
 */
function parseWasmId(stringId: string): bigint | null {
  try {
    return BigInt(stringId);
  } catch {
    return null;
  }
}
