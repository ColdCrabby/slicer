import { RuntimeHistorySession } from '../domain/history-models';
import { RuntimePreviewSource } from '../domain/preview-models';
import { RuntimeMeshInput, RuntimeSceneOp, RuntimeSceneSnapshot } from '../domain/scene-commands';
import { RuntimeSliceRequest, RuntimeSliceResult } from '../domain/slice-commands';
import { RuntimeCapabilities } from '../ports/runtime-capabilities';
import { RuntimeEventListener } from '../ports/runtime-events';
import { RuntimePort, RuntimeSubscription } from '../ports/runtime-port';
import { RuntimeSession } from './runtime-session';

export class RuntimeOrchestrator {
  constructor(
    private readonly runtime: RuntimePort,
    private readonly session: RuntimeSession,
  ) {}

  init(): Promise<void> {
    return this.runtime.init();
  }

  addMesh(input: RuntimeMeshInput): Promise<string[]> {
    return this.runtime.addMesh(input);
  }

  applySceneOps(ops: RuntimeSceneOp[]): Promise<void> {
    return this.runtime.applySceneOps(ops);
  }

  getSceneSnapshot(): Promise<RuntimeSceneSnapshot> {
    return this.runtime.getSceneSnapshot();
  }

  getHistory(): Promise<RuntimeHistorySession[]> {
    return this.runtime.getHistory();
  }

  clearHistory(): Promise<void> {
    return this.runtime.clearHistory();
  }

  getCapabilities(): RuntimeCapabilities {
    return this.runtime.getCapabilities();
  }

  async slice(request: RuntimeSliceRequest): Promise<RuntimeSliceResult> {
    this.session.setActiveSlice(request.sliceId);

    try {
      const result = await this.runtime.slice(request);
      this.session.setActiveSlice(null);
      return result;
    } catch (error) {
      this.session.setError(error instanceof Error ? error.message : 'Unknown slicing error');
      this.session.setActiveSlice(null);
      throw error;
    }
  }

  cancel(sliceId: string): Promise<void> {
    this.session.setActiveSlice(null);
    return this.runtime.cancel(sliceId);
  }

  getPreviewSource(sliceId: string): Promise<RuntimePreviewSource> {
    return this.runtime.getPreviewSource(sliceId);
  }

  onEvent(listener: RuntimeEventListener): RuntimeSubscription {
    return this.runtime.onEvent(listener);
  }

  dispose(): Promise<void> {
    return this.runtime.dispose();
  }
}
