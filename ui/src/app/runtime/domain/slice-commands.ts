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
  /**
   * Base64 PNG preview of the plate, rendered by the viewer and sent on every
   * slice. Absent only when the user turned thumbnails off or the capture
   * failed — no runtime can produce one on its own, because the picture is of
   * the user's camera, theme and filament colour.
   */
  thumbnailPngBase64?: string;
}

export interface RuntimeSliceResult {
  sliceId: string;
  layerCount: number;
  gcodeText?: string;
  downloadUrl?: string;
}
