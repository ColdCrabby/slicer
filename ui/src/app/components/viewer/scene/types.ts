import type { GizmoDelta } from '../gizmo';
import type { PaintBrushMode } from '../../../services/viewer-control';

export interface SceneSelectionHandlers {
  /** A bare click on a selectable object — `additive` for ctrl/⌘/shift. */
  select(id: string, additive: boolean): void;
  /** Click landed on empty space (deselect). */
  clearSelection(): void;
  /**
   * A context menu was asked for over the scene — a right-click, or the touch
   * and pen long-press that stands in for one. `id` is the object under the
   * pointer, or `null` when the press landed on empty bed.
   */
  contextMenu?(id: string | null, event: MouseEvent): void;
}

export interface SceneGizmoHandlers {
  /** Fired on each frame's incremental delta during a drag. */
  delta(ids: readonly string[], delta: GizmoDelta): void;
  /** Fired when the gesture finishes (pointer-up). Flush history here. */
  end(): void;
  /** Fired when a face has been picked in `pullToFloor` mode. */
  facePicked(objectId: string, faceIndex: number): void;
  /**
   * Fired for each brush dab while painting support in `'paint'` mode —
   * on contact and again on every rAF-throttled sample while the pointer
   * drags. `worldPoint` is where the raycast hit, `seedFace` the facet it
   * hit; both are forwarded to the `PaintSupport` op unchanged.
   */
  paintDab(
    objectId: string,
    seedFace: number,
    worldPoint: [number, number, number],
    radius: number,
    mode: PaintBrushMode,
  ): void;
  /** Fired once when a paint stroke finishes (pointer-up/cancel). Flush history here. */
  paintEnd(): void;
}

export type ViewerView = 'perspective' | 'ortho';
