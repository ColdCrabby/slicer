import { Injectable, inject } from '@angular/core';
import { SceneEngine, SceneOp, SceneSnapshot } from '../scene-engine';
import { SceneHistory } from '../scene-history/scene-history';

const COMMIT_DEBOUNCE_MS = 1000;

/**
 * Single public dispatch point for all scene operations.
 *
 * Wraps `SceneEngine.apply` with a gesture-batching layer:
 *
 * 1. The op is forwarded to the WASM engine **immediately** so real-time
 *    feedback (drag, gizmo movement) is never delayed.
 * 2. A `gestureStart` snapshot is captured before the first op of a gesture.
 * 3. Every op resets a 1-second debounce timer. When the timer fires
 *    (user paused for ≥1 s), the resulting state is pushed to `SceneHistory`.
 *
 * On the very first gesture commit the baseline snapshot is seeded into the
 * history stack so the user can always undo back to the initial state.
 *
 * All callers (viewer, gizmos, panels) should inject and use **this class**
 * instead of calling `SceneEngine.apply` directly. Initialisation
 * paths (`ready()`, `addMesh()`, `resetWithBed()`) still go directly to
 * `SceneEngine` since they are not undoable ops.
 */
@Injectable({ providedIn: 'root' })
export class SceneCommand {
  private readonly engine = inject(SceneEngine);
  private readonly history = inject(SceneHistory);

  private gestureStart: SceneSnapshot | null = null;
  private debounceTimer: ReturnType<typeof setTimeout> | null = null;

  /**
   * Apply a scene op.
   *
   * Accepts any `SceneOp` variant — translate, rotate, scale,
   * place_face_on_floor, center_on_bed, drop_to_floor, remove,
   * set_transform, auto_orient. History is op-type-agnostic; all ops are
   * batched identically by the before/after snapshot mechanism.
   */
  apply(op: SceneOp): void {
    if (this.gestureStart === null) {
      this.gestureStart = this.engine.snapshot();
    }

    this.engine.apply(op);
    this.scheduleCommit();
  }

  /**
   * Immediately flush any in-progress gesture to history without waiting
   * for the debounce timer.
   *
   * Call this when the user explicitly finishes an interaction (e.g.
   * pointer-up on a gizmo) to guarantee the history entry is committed
   * synchronously rather than after the idle timeout.
   */
  flush(): void {
    this.cancelTimer();
    this.commitGesture();
  }

  private scheduleCommit(): void {
    this.cancelTimer();
    this.debounceTimer = setTimeout(() => {
      this.debounceTimer = null;
      this.commitGesture();
    }, COMMIT_DEBOUNCE_MS);
  }

  private cancelTimer(): void {
    if (this.debounceTimer !== null) {
      clearTimeout(this.debounceTimer);
      this.debounceTimer = null;
    }
  }

  private commitGesture(): void {
    if (this.gestureStart === null) {
      return;
    }

    const before = this.gestureStart;
    const after = this.engine.snapshot();
    this.gestureStart = null;

    // Seed the stack with the baseline on the very first commit so the
    // user can always undo back to the state before any edits.
    if (this.history.entryCount() === 0) {
      this.history.push(before);
    }

    this.history.push(after);
  }
}
