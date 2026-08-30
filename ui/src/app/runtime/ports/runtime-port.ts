import { RuntimeHistorySession } from '../domain/history-models';
import { RuntimePreviewSource } from '../domain/preview-models';
import { RuntimeMeshInput, RuntimeSceneOp, RuntimeSceneSnapshot } from '../domain/scene-commands';
import { RuntimeSliceRequest, RuntimeSliceResult } from '../domain/slice-commands';
import { RuntimeCapabilities } from './runtime-capabilities';
import { RuntimeEventListener } from './runtime-events';

export interface RuntimeSubscription {
  unsubscribe(): void;
}

export interface RuntimePort {
  init(): Promise<void>;
  getCapabilities(): RuntimeCapabilities;
  /** Open a native OS file-picker and return a fully-populated mesh input.
   *  Only implemented by runtimes that support native file access (e.g. Tauri).
   *  Returns `null` when the user cancels the dialog. */
  openFilePicker?(): Promise<RuntimeMeshInput | null>;
  /** Add a mesh, returning the id of every scene object it created.
   *  A multi-part file (3MF) yields one id per part; STL/OBJ yield one. */
  addMesh(input: RuntimeMeshInput): Promise<string[]>;
  applySceneOps(ops: RuntimeSceneOp[]): Promise<void>;
  getSceneSnapshot(): Promise<RuntimeSceneSnapshot>;
  getHistory(): Promise<RuntimeHistorySession[]>;
  /** Drop all persisted slicing history (and, where applicable, the G-code
   *  cache). No-op on runtimes that keep no history (web/wasm). */
  clearHistory(): Promise<void>;
  slice(request: RuntimeSliceRequest): Promise<RuntimeSliceResult>;
  cancel(sliceId: string): Promise<void>;
  getPreviewSource(sliceId: string): Promise<RuntimePreviewSource>;
  onEvent(listener: RuntimeEventListener): RuntimeSubscription;
  dispose(): Promise<void>;
}
