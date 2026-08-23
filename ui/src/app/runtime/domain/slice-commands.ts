import { RuntimeMeshInput, RuntimeSceneSnapshot } from './scene-commands';
import type { ProfileSelection } from '../../../generated/slicer-engine-ws-client-message-v1';

export interface RuntimeSliceRequest {
  sliceId: string;
  request_uuid?: string;
  model?: RuntimeMeshInput;
  scene?: RuntimeSceneSnapshot;
  /** Legacy pre-flattened parameters (used by the local WASM/native runtimes). */
  settings: Record<string, unknown>;
  /**
   * Structured profile selection + sparse override diff. Preferred by the
   * server path: the engine resolves it. The three profiles are already in the
   * engine's own shape, so there is no mapping.
   */
  profiles?: ProfileSelection;
}

export interface RuntimeSliceResult {
  sliceId: string;
  layerCount: number;
  gcodeText?: string;
  downloadUrl?: string;
}
