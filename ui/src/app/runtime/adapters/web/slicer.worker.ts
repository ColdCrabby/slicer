/// <reference lib="webworker" />

import init, { SceneHandle } from '../../../../generated/scene-wasm/scene_engine';
import type {
  SlicerWorkerRequest,
  WasmSliceEvent,
  WorkerProfileSelection,
  WorkerSliceObject,
} from './slicer-worker-protocol';

interface LocalSliceResult {
  gcode: string;
  layer_count: number;
}

type SceneHandleWithEvents = SceneHandle & {
  sliceGcodeWithEvents?: (
    params: Record<string, unknown>,
    callback: (event: WasmSliceEvent) => void,
  ) => LocalSliceResult;
};

const DEFAULT_BED = {
  width: 220,
  depth: 220,
  height: 250,
  origin_offset_x: 0,
  origin_offset_y: 0,
};

let wasmUrl = 'scene_engine_bg.wasm';
let wasmReady: Promise<void> | null = null;
/**
 * The engine's own profile resolver, claimed when the module is initialised.
 *
 * Looked up dynamically rather than imported by name — the same idiom
 * `BrowserProfilePersistence` uses for `exportProfileLibrary`. Only the
 * `web-slicer` build exports it, so a static import would be a name the
 * checked-in scene-only declarations do not have. That is also the build which
 * exports `sliceGcodeWithEvents`: a bundle that can slice can always resolve.
 */
let resolveSliceParams: ((selection: WorkerProfileSelection) => Record<string, unknown>) | null =
  null;

self.addEventListener('message', (event: MessageEvent<SlicerWorkerRequest>) => {
  void handleMessage(event.data);
});

async function handleMessage(message: SlicerWorkerRequest): Promise<void> {
  try {
    switch (message.type) {
      case 'init':
        await ensureWasm(message.wasmUrl);
        self.postMessage({ type: 'ready' });
        break;
      case 'slice':
        await ensureWasm();
        runSlice(message.sliceId, message.profiles, message.objects, message.thumbnailPngBase64);
        break;
    }
  } catch (error) {
    self.postMessage({
      type: 'error',
      sliceId: message.type === 'slice' ? message.sliceId : undefined,
      message: messageOf(error),
    });
  }
}

async function ensureWasm(nextUrl?: string): Promise<void> {
  if (nextUrl) {
    wasmUrl = nextUrl;
  }

  if (!wasmReady) {
    wasmReady = (async () => {
      const wasm = (await import('../../../../generated/scene-wasm/scene_engine')) as unknown as {
        resolveSliceParams?: (selection: WorkerProfileSelection) => Record<string, unknown>;
      };
      await init({ module_or_path: wasmUrl });
      resolveSliceParams = wasm.resolveSliceParams ?? null;
    })();
  }

  return wasmReady;
}

function runSlice(
  sliceId: string,
  profiles: WorkerProfileSelection,
  objects: WorkerSliceObject[],
  thumbnailPngBase64?: string,
): void {
  const totalStart = performance.now();
  emitPhaseStart(sliceId, 'total');

  let handle: SceneHandle | null = null;
  try {
    if (objects.length === 0) {
      throw new Error('Cannot slice an empty scene.');
    }

    // Compose the profile stack here, in the engine, rather than sending a
    // pre-flattened blob across from the UI. The request carries only the
    // user's deviations; everything inherited comes from the profiles.
    if (!resolveSliceParams) {
      throw new Error(
        'This wasm bundle does not include the profile resolver. Rebuild with pnpm run hydrate:web-slicer.',
      );
    }
    const settings = resolveSliceParams(profiles);
    // Rendered by the viewer on the main thread and handed over as-is. The
    // engine never makes one; it only embeds what it is given.
    if (thumbnailPngBase64) {
      settings['thumbnail_png_base64'] = thumbnailPngBase64;
    }

    const meshLoadStart = performance.now();
    emitPhaseStart(sliceId, 'mesh_load');
    handle = new SceneHandle(DEFAULT_BED);
    for (const object of objects) {
      addObject(handle, object);
    }
    emitPhaseEnd(sliceId, 'mesh_load', elapsedSince(meshLoadStart));

    const slicer = handle as SceneHandleWithEvents;
    if (typeof slicer.sliceGcodeWithEvents !== 'function') {
      throw new Error(
        'This wasm bundle does not include worker slicing events. Rebuild with pnpm run hydrate:web-slicer.',
      );
    }

    const result = slicer.sliceGcodeWithEvents(settings, (event: WasmSliceEvent) =>
      forwardWasmEvent(sliceId, event),
    );
    handle.free();
    handle = null;

    emitPhaseEnd(sliceId, 'total', elapsedSince(totalStart));
    self.postMessage({
      type: 'slice-complete',
      sliceId,
      layerCount: result.layer_count,
      gcode: result.gcode,
    });
  } catch (error) {
    emitPhaseEnd(sliceId, 'total', elapsedSince(totalStart));
    self.postMessage({ type: 'error', sliceId, message: messageOf(error) });
  } finally {
    handle?.free();
  }
}

function addObject(handle: SceneHandle, object: WorkerSliceObject): void {
  // A multi-part file (3MF) adds every part; this scene entry is only one of
  // them, so keep the requested part and drop the rest. Without this each
  // entry would contribute the whole file and print every part N times.
  const ids = Array.from(handle.addMesh(object.name, object.format, object.bytes), (id) =>
    BigInt(id as unknown as string | number),
  );
  const partIndex = object.partIndex ?? 0;
  const id = ids[partIndex];
  if (id === undefined) {
    throw new Error(
      `'${object.name}' has no object at index ${partIndex} (it contains ${ids.length})`,
    );
  }
  const surplus = ids.filter((other) => other !== id);
  if (surplus.length > 0) {
    handle.applyOp({ op: 'RemoveMany', args: { ids: surplus } });
  }
  handle.applyOp({
    op: 'SetTransform',
    args: {
      id,
      translation: object.transform.translation,
      euler_xyz_deg: object.transform.euler_xyz_deg,
      scale: object.transform.scale,
    },
  });
  if (object.supportPaint) {
    handle.applyOp({
      op: 'SetSupportPaint',
      args: { id, encoded: object.supportPaint },
    });
  }
}

function forwardWasmEvent(sliceId: string, event: WasmSliceEvent): void {
  switch (event.type) {
    case 'log':
      self.postMessage({ type: 'log', sliceId, level: event.level, message: event.message });
      break;
    case 'phase':
      if (event.event === 'start') {
        emitPhaseStart(sliceId, event.phase, event.object, event.object_count);
      } else {
        emitPhaseEnd(sliceId, event.phase, event.elapsed_ms ?? 0, event.object, event.object_count);
      }
      break;
    case 'progress':
      self.postMessage({
        type: 'progress',
        sliceId,
        currentLayer: event.current_layer,
        totalLayers: event.total_layers,
      });
      break;
  }
}

function emitPhaseStart(
  sliceId: string,
  phase: string,
  object?: number,
  objectCount?: number,
): void {
  self.postMessage({ type: 'phase-start', sliceId, phase, object, objectCount });
}

function emitPhaseEnd(
  sliceId: string,
  phase: string,
  elapsedMs: number,
  object?: number,
  objectCount?: number,
): void {
  self.postMessage({ type: 'phase-end', sliceId, phase, elapsedMs, object, objectCount });
}

function elapsedSince(start: number): number {
  return Math.max(0, Math.round(performance.now() - start));
}

function messageOf(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === 'string') {
    return error;
  }
  return 'Unknown worker slicing error';
}
