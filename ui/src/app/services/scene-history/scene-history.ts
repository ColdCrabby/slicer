import { Injectable, computed, inject, signal } from '@angular/core';
import { Logger } from '../logger';
import { SceneEngine, SceneSnapshot } from '../scene-engine';

const MAX_ENTRIES = 50;

/**
 * Linear undo/redo stack where each entry is a complete `SceneSnapshot`.
 *
 * The stack is a flat list of states: `[s0, s1, s2, ...]`. The cursor points
 * at the currently active state. Undo steps the cursor back and restores the
 * previous snapshot; Redo steps it forward. Any new push while the cursor is
 * not at the tail trims the redo branch, enforcing a linear history.
 *
 * The initial baseline snapshot (`s0`) is pushed by `SceneCommand` on the
 * very first gesture so undoing back to "nothing changed" is always possible.
 *
 * Restoration issues `set_transform` ops for every object present in the
 * target snapshot, and `remove` ops for objects that should no longer exist.
 * Re-adding objects whose mesh bytes are no longer in memory is deferred to a
 * future stage.
 */
@Injectable({ providedIn: 'root' })
export class SceneHistory {
  private readonly engine = inject(SceneEngine);
  private readonly log = inject(Logger).scope('SceneHistory');

  private readonly stack = signal<SceneSnapshot[]>([]);
  private readonly cursor = signal(-1);

  readonly canUndo = computed(() => this.cursor() > 0);
  readonly canRedo = computed(() => this.cursor() < this.stack().length - 1);

  /** Number of snapshots currently stored. */
  readonly entryCount = computed(() => this.stack().length);

  /**
   * Push a new snapshot. Any redo-branch entries above the current cursor
   * are discarded. The stack is capped at MAX_ENTRIES entries,
   * dropping the oldest when necessary.
   */
  push(snapshot: SceneSnapshot): void {
    this.stack.update((prev) => {
      const trimmed = prev.slice(0, this.cursor() + 1);
      const next = [...trimmed, snapshot];
      return next.length > MAX_ENTRIES ? next.slice(next.length - MAX_ENTRIES) : next;
    });
    this.cursor.update((c) => Math.min(c + 1, MAX_ENTRIES - 1));

    const newCursor = this.cursor();
    const total = this.stack().length;
    this.log.debug('push', {
      cursor: newCursor,
      total,
      snapshot,
    });
  }

  /** Step back one entry and restore that snapshot. */
  undo(): void {
    if (!this.canUndo()) {
      this.log.debug('undo -- nothing to undo');
      return;
    }
    const from = this.cursor();
    this.cursor.update((c) => c - 1);
    const to = this.cursor();
    this.log.info('undo', { from, to, total: this.stack().length });
    this.restoreSnapshot(this.stack()[to]);
  }

  /** Step forward one entry and restore that snapshot. */
  redo(): void {
    if (!this.canRedo()) {
      this.log.debug('redo -- nothing to redo');
      return;
    }
    const from = this.cursor();
    this.cursor.update((c) => c + 1);
    const to = this.cursor();
    this.log.info('redo', { from, to, total: this.stack().length });
    this.restoreSnapshot(this.stack()[to]);
  }

  /** Clear the entire stack. */
  clear(): void {
    this.log.debug('clear', { had: this.stack().length });
    this.stack.set([]);
    this.cursor.set(-1);
  }

  private restoreSnapshot(snapshot: SceneSnapshot): void {
    const currentIds = new Set(this.engine.objects().map((o) => String(o.id)));

    for (const id of currentIds) {
      if (!snapshot.objects.some((o) => String(o.id) === id)) {
        this.engine.apply({ op: 'Remove', args: { id: BigInt(id) } });
      }
    }

    for (const obj of snapshot.objects) {
      if (!currentIds.has(String(obj.id))) {
        // Mesh bytes not available -- skip re-add until file tracking is wired.
        continue;
      }
      this.engine.apply({
        op: 'SetTransform',
        args: {
          id: obj.id,
          translation: obj.translation,
          euler_xyz_deg: obj.euler_xyz_deg,
          scale: obj.scale,
        },
      });
      // Paint is per-facet, not part of the transform, so it needs its own
      // replay — otherwise undoing any edit would silently erase every
      // painted region on the plate.
      this.engine.apply({
        op: 'SetSupportPaint',
        args: { id: obj.id, encoded: obj.support_paint },
      });
    }
  }
}
