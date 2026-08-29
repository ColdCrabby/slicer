import { Subscription } from 'rxjs';
import {
  ClientMessage,
  SceneObjectSliceDto,
  SlicingParams,
} from '../../../../generated/slicer-engine-ws-client-message-v1';
import {
  ServerMessage,
  SessionSummary,
} from '../../../../generated/slicer-engine-ws-server-message-v1';
import { environment } from '../../../../environments/environment';
import { SceneEngine } from '../../../services/scene-engine';
import { SlicerConnection } from '../../../services/slicer-connection';
import { SlicerFile } from '../../../services/slicer-file';
import { RuntimeHistorySession } from '../../domain/history-models';
import { RuntimePreviewSource } from '../../domain/preview-models';
import {
  RuntimeMeshInput,
  RuntimeSceneOp,
  RuntimeSceneSnapshot,
} from '../../domain/scene-commands';
import { RuntimeSliceRequest, RuntimeSliceResult } from '../../domain/slice-commands';
import { RuntimeEventBus } from '../../infrastructure/event-bus';
import { RuntimeCapabilities } from '../../ports/runtime-capabilities';
import { RuntimeError } from '../../ports/runtime-errors';
import { RuntimeEventListener } from '../../ports/runtime-events';
import { RuntimePort, RuntimeSubscription } from '../../ports/runtime-port';
import { toSliceDtos } from './scene-slice-dto';

const CLOUD_SLICE_TIMEOUT_MS = 30 * 60 * 1000;
const CLOUD_HISTORY_TIMEOUT_MS = 10 * 1000;

const CLOUD_CAPABILITIES: RuntimeCapabilities = {
  supportsLocalSlicing: false,
  supportsRemoteJobs: true,
  supportsStreamingProgress: true,
  supportsSceneSnapshotPull: true,
};

export class CloudRuntime implements RuntimePort {
  private readonly bus = new RuntimeEventBus();
  private subscription: Subscription | null = null;
  private pendingSliceId: string | null = null;
  private pendingResolve: ((result: RuntimeSliceResult) => void) | null = null;
  private pendingReject: ((error: Error) => void) | null = null;
  private pendingTimeout: ReturnType<typeof setTimeout> | null = null;
  private pendingHistoryResolve: ((sessions: RuntimeHistorySession[]) => void) | null = null;
  private pendingHistoryReject: ((error: Error) => void) | null = null;
  private pendingHistoryTimeout: ReturnType<typeof setTimeout> | null = null;
  private readonly previewBySlice = new Map<string, string>();
  private initialized = false;

  constructor(
    private readonly apiUrl: string,
    private readonly ws: SlicerConnection,
    private readonly slicerFile: SlicerFile,
    private readonly sceneEngine: SceneEngine,
  ) {}

  async init(): Promise<void> {
    if (!this.subscription) {
      this.subscription = this.ws.messages$.subscribe((message) => this.handleMessage(message));
    }

    this.initialized = true;
    if (this.ws.isConnected()) {
      this.bus.emit({ type: 'connected', mode: 'cloud' });
    }
  }

  getCapabilities(): RuntimeCapabilities {
    return CLOUD_CAPABILITIES;
  }

  async addMesh(input: RuntimeMeshInput): Promise<string> {
    this.requireReady();
    if (!input.bytes) {
      throw new Error(`Cloud runtime requires bytes for '${input.fileName}'`);
    }
    const uploadBytes = new Uint8Array(input.bytes);
    const file = new File([uploadBytes], input.fileName, {
      type: 'application/octet-stream',
    });
    // Attach to the open workplate so a plate accumulates models. `upload`
    // appends to the file registry rather than replacing it.
    const upload = await this.slicerFile.upload(file);
    const fileId = upload.ofids[0] ?? '';
    // Stamp the upload id onto the scene object so slicing resolves this
    // object to these exact bytes instead of guessing by position.
    const objectId = this.sceneEngine.addMesh(
      input.fileName,
      input.format,
      uploadBytes,
      fileId || undefined,
    );
    return objectId.toString();
  }

  async applySceneOps(ops: RuntimeSceneOp[]): Promise<void> {
    this.requireReady();
    const payload: ClientMessage = {
      type: 'Scene',
      ops: ops.map((op) => {
        const id = Number(op.id);
        switch (op.op) {
          case 'remove':
            return { op: 'Remove', args: { id } };
          case 'duplicate':
            return { op: 'Duplicate', args: { id, offset: op.offset ?? [0, 0, 0] } };
          case 'translate':
            return { op: 'Translate', args: { id, delta: op.delta } };
          case 'set_transform':
            return {
              op: 'SetTransform',
              args: {
                id,
                translation: op.translation,
                euler_xyz_deg: op.euler_xyz_deg,
                scale: op.scale,
              },
            };
          case 'rotate':
            return { op: 'Rotate', args: { id, axis: op.axis, degrees: op.degrees } };
          case 'scale':
            return { op: 'Scale', args: { id, factors: op.factors } };
          case 'center_on_bed':
            return { op: 'CenterOnBed', args: { id } };
          case 'drop_to_floor':
            return { op: 'DropToFloor', args: { id } };
          case 'place_face_on_floor':
            return { op: 'PlaceFaceOnFloor', args: { id, face_index: op.face_index } };
        }
      }),
      options: { gravity: false },
    };

    this.ws.send(payload);
  }

  async getSceneSnapshot(): Promise<RuntimeSceneSnapshot> {
    this.requireReady();
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
        source_id: object.source_id,
      })),
    };
  }

  async getHistory(): Promise<RuntimeHistorySession[]> {
    this.requireReady();

    if (!this.ws.isConnected()) {
      throw new Error(this.ws.lastError() || 'WebSocket not connected');
    }

    this.ws.send({ type: 'ListSessions' });

    return new Promise<RuntimeHistorySession[]>((resolve, reject) => {
      this.pendingHistoryResolve = resolve;
      this.pendingHistoryReject = reject;
      this.pendingHistoryTimeout = setTimeout(() => {
        this.rejectPendingHistory(new Error('Cloud history request timed out.'));
      }, CLOUD_HISTORY_TIMEOUT_MS);
    });
  }

  async clearHistory(): Promise<void> {
    this.requireReady();
    const response = await fetch(`${environment.apiUrl}/history`, { method: 'DELETE' });
    if (!response.ok) {
      throw new Error(`DELETE /history failed (${response.status})`);
    }
  }

  async slice(request: RuntimeSliceRequest): Promise<RuntimeSliceResult> {
    this.requireReady();
    if (!this.ws.isConnected()) {
      throw new Error(this.ws.lastError() || 'WebSocket not connected');
    }

    const requestUuid = request.request_uuid ?? this.slicerFile.requestUuid();
    const fileIds = this.slicerFile.fileIds();
    if (!requestUuid || fileIds.length === 0) {
      throw new Error('Missing uploaded file context. Upload a file before slicing.');
    }

    const scene = this.buildSceneSnapshot(fileIds, request.scene);
    const payload: ClientMessage = request.profiles
      ? {
          type: 'Slice',
          request_uuid: requestUuid,
          scene,
          profiles: request.profiles,
        }
      : {
          type: 'Slice',
          request_uuid: requestUuid,
          scene,
          settings: request.settings as SlicingParams,
        };

    this.pendingSliceId = request.sliceId;
    this.ws.send(payload);
    this.bus.emit({ type: 'phase-start', sliceId: request.sliceId, phase: 'total' });

    return new Promise<RuntimeSliceResult>((resolve, reject) => {
      this.pendingResolve = resolve;
      this.pendingReject = reject;
      this.pendingTimeout = setTimeout(() => {
        this.rejectPending(new Error('Cloud slice timed out.'));
      }, CLOUD_SLICE_TIMEOUT_MS);
    });
  }

  async cancel(_sliceId: string): Promise<void> {
    this.requireReady();
    this.ws.send({ type: 'Reset' });
    this.rejectPending(new Error('Slice canceled.'));
  }

  async getPreviewSource(sliceId: string): Promise<RuntimePreviewSource> {
    this.requireReady();
    const url = this.previewBySlice.get(sliceId);
    if (url) {
      return { kind: 'download-url', url };
    }
    return { kind: 'none' };
  }

  onEvent(listener: RuntimeEventListener): RuntimeSubscription {
    return this.bus.subscribe(listener);
  }

  async dispose(): Promise<void> {
    this.subscription?.unsubscribe();
    this.subscription = null;
    this.initialized = false;
    this.clearPendingTimeout();
    this.clearPendingHistoryTimeout();
    this.bus.clear();
  }

  private handleMessage(msg: ServerMessage): void {
    switch (msg.type) {
      case 'Connected':
        this.bus.emit({ type: 'connected', mode: 'cloud', serverVersion: msg.version });
        this.bus.emit({
          type: 'log',
          level: 'info',
          message: `Connected to slicer-engine v${msg.version}`,
        });
        break;
      case 'Log':
        this.bus.emit({
          type: 'log',
          level: this.toRuntimeLogLevel(msg.level),
          message: msg.message,
        });
        break;
      case 'PhaseMarker':
        if (!this.pendingSliceId) {
          return;
        }
        if (msg.event === 'start') {
          this.bus.emit({
            type: 'phase-start',
            sliceId: this.pendingSliceId,
            phase: msg.phase,
          });
        }
        if (msg.event === 'end') {
          this.bus.emit({
            type: 'phase-end',
            sliceId: this.pendingSliceId,
            phase: msg.phase,
            elapsedMs: msg.elapsed_ms ?? undefined,
          });
        }
        break;
      case 'Progress':
        if (!this.pendingSliceId) {
          return;
        }
        this.bus.emit({
          type: 'progress',
          sliceId: this.pendingSliceId,
          currentLayer: msg.current_layer,
          totalLayers: msg.total_layers,
        });
        break;
      case 'SliceComplete': {
        if (!this.pendingSliceId) {
          return;
        }
        const sliceId = this.pendingSliceId;
        const downloadUrl = msg.download_url.startsWith('/')
          ? `${this.apiUrl}${msg.download_url}`
          : msg.download_url;
        this.previewBySlice.set(sliceId, downloadUrl);
        this.bus.emit({ type: 'phase-end', sliceId, phase: 'total', elapsedMs: 0 });
        this.bus.emit({
          type: 'slice-complete',
          sliceId,
          layerCount: msg.layer_count,
          downloadUrl,
        });
        this.resolvePending({
          sliceId,
          layerCount: msg.layer_count,
          downloadUrl,
        });
        break;
      }
      case 'Error': {
        const error = new Error(msg.message);
        this.bus.emit({
          type: 'error',
          error: {
            code: 'transport_error',
            message: msg.message,
            cause: error,
          },
        });
        this.rejectPending(error);
        this.rejectPendingHistory(error);
        break;
      }
      case 'SessionsList': {
        this.resolvePendingHistory(msg.sessions.map((session) => this.mapSession(session)));
        break;
      }
    }
  }

  private buildSceneSnapshot(
    uploadFileIds: string[],
    requestScene?: RuntimeSceneSnapshot,
  ): SceneObjectSliceDto[] {
    return toSliceDtos(requestScene?.objects ?? this.getSceneObjects(), uploadFileIds);
  }

  private getSceneObjects(): RuntimeSceneSnapshot['objects'] {
    return this.sceneEngine.objects().map((object) => ({
      id: object.id.toString(),
      name: object.name,
      translation: object.translation,
      euler_xyz_deg: object.euler_xyz_deg,
      scale: object.scale,
      triangle_count: object.triangle_count,
      world_aabb: object.world_aabb,
      source_id: object.source_id,
    }));
  }

  private toRuntimeLogLevel(level: string): 'debug' | 'info' | 'warn' | 'error' {
    if (level === 'debug' || level === 'info' || level === 'warn' || level === 'error') {
      return level;
    }
    return 'info';
  }

  private resolvePending(result: RuntimeSliceResult): void {
    this.pendingResolve?.(result);
    this.pendingResolve = null;
    this.pendingReject = null;
    this.pendingSliceId = null;
    this.clearPendingTimeout();
  }

  private rejectPending(error: Error): void {
    this.pendingReject?.(error);
    this.pendingResolve = null;
    this.pendingReject = null;
    this.pendingSliceId = null;
    this.clearPendingTimeout();
  }

  private clearPendingTimeout(): void {
    if (this.pendingTimeout) {
      clearTimeout(this.pendingTimeout);
      this.pendingTimeout = null;
    }
  }

  private mapSession(session: SessionSummary): RuntimeHistorySession {
    return {
      request_uuid: session.request_uuid,
      created_at: session.created_at,
      original_filename: session.original_filename,
      layer_count: session.layer_count,
      download_url: session.download_url,
    };
  }

  private resolvePendingHistory(sessions: RuntimeHistorySession[]): void {
    this.pendingHistoryResolve?.(sessions);
    this.pendingHistoryResolve = null;
    this.pendingHistoryReject = null;
    this.clearPendingHistoryTimeout();
  }

  private rejectPendingHistory(error: Error): void {
    this.pendingHistoryReject?.(error);
    this.pendingHistoryResolve = null;
    this.pendingHistoryReject = null;
    this.clearPendingHistoryTimeout();
  }

  private clearPendingHistoryTimeout(): void {
    if (this.pendingHistoryTimeout) {
      clearTimeout(this.pendingHistoryTimeout);
      this.pendingHistoryTimeout = null;
    }
  }

  private requireReady(): void {
    if (!this.initialized) {
      const error: RuntimeError = {
        code: 'not_ready',
        message: 'Cloud runtime has not been initialized.',
      };
      this.bus.emit({ type: 'error', error });
      throw new Error(error.message);
    }
  }
}
