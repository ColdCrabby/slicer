import { RuntimeMeshInput, RuntimeSceneSnapshot } from './scene-commands';
import type { ProfileSelection } from '../../../generated/slicer-engine-ws-client-message-v1';

export interface RuntimeSliceRequest {
  sliceId: string;
  request_uuid?: string;
  model?: RuntimeMeshInput;
  scene?: RuntimeSceneSnapshot;
  /**
   * The three active profiles plus the user's sparse override diff — the whole
   * parameter half of a slice request.
   *
   * Every runtime resolves it with the engine's own `profiles::resolve`: the
   * server in `ws_session`, the desktop app in its Tauri bridge, the browser in
   * the wasm worker. There is deliberately no pre-flattened alternative here,
   * because the moment one exists the client starts composing profiles itself
   * and the two answers drift.
   */
  profiles: ProfileSelection;
}

export interface RuntimeSliceResult {
  sliceId: string;
  layerCount: number;
  gcodeText?: string;
  downloadUrl?: string;
}
