import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { KeyboardShortcuts } from '../../services/keyboard-shortcuts/keyboard-shortcuts';
import { SceneCommand } from '../../services/scene-command/scene-command';
import { SceneEngine, type SceneObjectSnapshot } from '../../services/scene-engine';
import { ViewerControl } from '../../services/viewer-control';
import { Icon } from '../../shared/icon/icon';
import { TooltipDirective } from '../../shared/tooltip/tooltip.directive';
import { NumberInput } from '../../ui/number-input/number-input';
import { Segmented, type SegmentOption } from '../../ui/segmented/segmented';

/** Which numeric readout the scale mode edits. */
type ScaleUnit = 'percent' | 'size';

/** A single X/Y/Z axis index. */
type Axis = 0 | 1 | 2;

const AXES: readonly Axis[] = [0, 1, 2];
const AXIS_LABELS = ['X', 'Y', 'Z'] as const;

const SCALE_UNIT_OPTIONS: readonly SegmentOption[] = [
  {
    value: 'percent',
    label: 'Scale',
    description: 'Set each axis as a percentage of the original',
  },
  { value: 'size', label: 'Size', description: 'Set the bounding-box dimensions in millimetres' },
];

/** Round a 3-vector to `decimals` places for display (never for op math). */
function round3(v: readonly number[], decimals = 2): [number, number, number] {
  const f = 10 ** decimals;
  return [Math.round(v[0] * f) / f, Math.round(v[1] * f) / f, Math.round(v[2] * f) / f];
}

/**
 * The value shown for an axis across the selection.
 *
 * When every object agrees this is simply that value. When they differ there
 * is no honest single number, so the first object's is shown — typing into the
 * field then sets *every* selected object, which is the useful operation
 * ("make them all 45°").
 */
function representativeValue(values: readonly number[]): number {
  return values[0] ?? 0;
}

/** Combined world-space AABB of several objects. */
function unionAabb(objects: readonly SceneObjectSnapshot[]): [number[], number[]] | null {
  if (objects.length === 0) {
    return null;
  }
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (const o of objects) {
    const [lo, hi] = o.world_aabb;
    for (let i = 0; i < 3; i++) {
      min[i] = Math.min(min[i], lo[i]);
      max[i] = Math.max(max[i], hi[i]);
    }
  }
  return [min, max];
}

/**
 * Contextual transform sub-settings for the current selection.
 *
 * Hangs under the 3D toolbar's object-mode selector and mirrors the
 * OrcaSlicer transform toolbar: whichever manipulation mode is active
 * (translate / rotate / scale) reveals precise numeric fields for that
 * operation. Values are read live from the WASM scene engine snapshot (so
 * gizmo drags update the fields), and every edit is dispatched as an absolute
 * `SetTransform` op through {@link SceneCommand} so it participates in
 * undo/redo exactly like a gizmo gesture.
 *
 * **It edits a selection, not one object.** A plate holds several models, so
 * the panel works for any non-empty selection and the whole batch shares one
 * undo entry. The two kinds of edit differ, because only one reading is
 * unsurprising in each case:
 *
 * - **Position moves the group.** The fields show the selection's combined
 *   bounding-box centre and an edit applies the *difference* to every object,
 *   so a spread-out arrangement keeps its layout instead of collapsing onto
 *   one spot.
 * - **Rotation and scale are set per object.** Each object turns about its own
 *   centre / scales in place, so "make them all 45°" does what it says.
 */
@Component({
  selector: 'nexus-transform-panel',
  standalone: true,
  imports: [NumberInput, Segmented, Icon, TooltipDirective],
  templateUrl: './transform-panel.html',
  styleUrl: './transform-panel.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class TransformPanel {
  private readonly viewerControl = inject(ViewerControl);
  private readonly sceneEngine = inject(SceneEngine);
  private readonly sceneCommand = inject(SceneCommand);

  protected readonly axes = AXES;
  protected readonly axisLabels = AXIS_LABELS;
  protected readonly scaleUnitOptions = SCALE_UNIT_OPTIONS;

  /** Markdown details shown by the header info tooltip (block/persistent). */
  protected readonly stepHint = [
    '**Adjust a number field**',
    '',
    '- **Scroll** over a field to step it',
    '- **↑ / ↓** step a focused field',
    '- **Shift** — coarse step (×10)',
    `- **${inject(KeyboardShortcuts).isMac ? '⌥ Option' : 'Alt'}** — fine step (×0.1)`,
  ].join('\n');

  /** Active object mode, narrowed to the three transform modes (else null). */
  protected readonly mode = computed<'translate' | 'rotate' | 'scale' | null>(() => {
    const m = this.viewerControl.objectMode();
    return m === 'translate' || m === 'rotate' || m === 'scale' ? m : null;
  });

  /** Every selected object, in scene order. */
  protected readonly selection = computed<SceneObjectSnapshot[]>(() => {
    const ids = new Set(this.viewerControl.selectedObjectIds());
    if (ids.size === 0) {
      return [];
    }
    return this.sceneEngine.objects().filter((o) => ids.has(o.id));
  });

  /** How many objects the fields are editing. */
  protected readonly count = computed(() => this.selection().length);

  /** Label naming the batch, or empty for a single object. */
  protected readonly title = computed(() => (this.count() > 1 ? `${this.count()} objects` : ''));

  /** Whether the panel should be shown at all. */
  protected readonly visible = computed(() => this.mode() !== null && this.count() > 0);

  /** Which unit the scale mode edits (percent of original vs. absolute mm). */
  protected readonly scaleUnit = signal<ScaleUnit>('percent');

  protected setScaleUnit(value: string): void {
    this.scaleUnit.set(value as ScaleUnit);
  }

  /** When on, editing one scale/size axis scales the others proportionally. */
  protected readonly uniform = signal(true);

  /**
   * Live position (mm): the object's own translation for a single selection,
   * or the combined bounding-box centre for several — the anchor an edit
   * moves the whole group by.
   */
  protected readonly position = computed<[number, number, number]>(() => {
    const objects = this.selection();
    if (objects.length === 1) {
      return round3(objects[0].translation);
    }
    const box = unionAabb(objects);
    if (!box) {
      return [0, 0, 0];
    }
    const [min, max] = box;
    return round3([(min[0] + max[0]) / 2, (min[1] + max[1]) / 2, (min[2] + max[2]) / 2]);
  });

  /** Live Euler-XYZ rotation (degrees) of the selection. */
  protected readonly rotation = computed<[number, number, number]>(() => {
    const objects = this.selection();
    if (objects.length === 0) {
      return [0, 0, 0];
    }
    return round3(
      AXES.map((axis) => representativeValue(objects.map((o) => o.euler_xyz_deg[axis]))),
      1,
    );
  });

  /** Live per-axis scale expressed as a percentage of the original mesh. */
  protected readonly scalePercent = computed<[number, number, number]>(() => {
    const objects = this.selection();
    if (objects.length === 0) {
      return [100, 100, 100];
    }
    return round3(
      AXES.map((axis) => representativeValue(objects.map((o) => o.scale[axis])) * 100),
      1,
    );
  });

  /**
   * Live world-space bounding-box size (mm): the object's own for a single
   * selection, the combined extent for several.
   */
  protected readonly size = computed<[number, number, number]>(() => {
    const box = unionAabb(this.selection());
    if (!box) {
      return [0, 0, 0];
    }
    const [min, max] = box;
    return round3([max[0] - min[0], max[1] - min[1], max[2] - min[2]]);
  });

  // ---------------------------------------------------------------------------
  // Position
  // ---------------------------------------------------------------------------

  /**
   * Move the selection so the shown anchor lands on `value`.
   *
   * Applied as a delta, so several objects keep their relative layout instead
   * of stacking on one coordinate. For a single object the anchor *is* its
   * translation, so this is an exact absolute set.
   */
  protected setPosition(axis: Axis, value: number): void {
    const objects = this.selection();
    if (objects.length === 0) {
      return;
    }
    const delta = value - this.position()[axis];
    if (delta === 0) {
      return;
    }
    for (const o of objects) {
      const translation: [number, number, number] = [...o.translation];
      translation[axis] += delta;
      // Editing Z is an explicit vertical placement — never let gravity undo it.
      this.commitTransform(o, { translation }, { gravity: axis !== 2 });
    }
  }

  /** Centre the selection on the bed (X/Y), keeping Z. */
  protected centerOnBed(): void {
    for (const o of this.selection()) {
      this.sceneCommand.apply({ op: 'CenterOnBed', args: { id: o.id } });
    }
  }

  /** Drop the selection so its lowest point rests on the bed. */
  protected dropToFloor(): void {
    for (const o of this.selection()) {
      this.sceneCommand.apply({ op: 'DropToFloor', args: { id: o.id } });
    }
  }

  // ---------------------------------------------------------------------------
  // Rotation
  // ---------------------------------------------------------------------------

  protected setRotation(axis: Axis, value: number): void {
    for (const o of this.selection()) {
      const euler: [number, number, number] = [...o.euler_xyz_deg];
      euler[axis] = value;
      this.commitTransform(o, { euler });
    }
  }

  /** Reset all rotation to zero. */
  protected resetRotation(): void {
    for (const o of this.selection()) {
      this.commitTransform(o, { euler: [0, 0, 0] });
    }
  }

  // ---------------------------------------------------------------------------
  // Scale
  // ---------------------------------------------------------------------------

  protected setScalePercent(axis: Axis, value: number): void {
    const target = value / 100;
    for (const o of this.selection()) {
      this.applyScaleAxis(o, axis, target);
    }
  }

  /**
   * Resize so the shown dimension reads `value`.
   *
   * Each object is measured from its **own** bounding box, so a batch of
   * different-sized parts all end up that size rather than inheriting one
   * object's scale factor.
   */
  protected setSize(axis: Axis, value: number): void {
    for (const o of this.selection()) {
      const [min, max] = o.world_aabb;
      const cur = max[axis] - min[axis];
      if (cur <= 0) {
        continue;
      }
      // Convert the target dimension into an absolute per-axis scale factor.
      this.applyScaleAxis(o, axis, (o.scale[axis] * value) / cur);
    }
  }

  /** Reset scale on every axis to 100%. */
  protected resetScale(): void {
    for (const o of this.selection()) {
      this.commitTransform(o, { scale: [1, 1, 1] });
    }
  }

  protected toggleUniform(): void {
    this.uniform.update((v) => !v);
  }

  /**
   * Set one axis to an absolute scale factor. With the uniform lock on, the
   * ratio is applied to every axis so the object scales proportionally.
   */
  private applyScaleAxis(o: SceneObjectSnapshot, axis: Axis, target: number): void {
    const safeTarget = Math.max(0.001, target);
    let scale: [number, number, number];
    if (this.uniform()) {
      const ratio = safeTarget / o.scale[axis];
      scale = [o.scale[0] * ratio, o.scale[1] * ratio, o.scale[2] * ratio];
    } else {
      scale = [...o.scale];
      scale[axis] = safeTarget;
    }
    this.commitTransform(o, { scale });
  }

  // ---------------------------------------------------------------------------
  // Dispatch
  // ---------------------------------------------------------------------------

  /**
   * Dispatch an absolute `SetTransform`, keeping the components not being
   * edited. Batched through {@link SceneCommand} so a burst of edits collapses
   * into a single undo entry (matching a gizmo drag). When `gravity` is
   * allowed and the gravity toggle is on, the object is re-dropped to the floor
   * as part of the same gesture.
   */
  private commitTransform(
    o: SceneObjectSnapshot,
    changes: {
      translation?: [number, number, number];
      euler?: [number, number, number];
      scale?: [number, number, number];
    },
    options: { gravity?: boolean } = {},
  ): void {
    this.sceneCommand.apply({
      op: 'SetTransform',
      args: {
        id: o.id,
        translation: changes.translation ?? [...o.translation],
        euler_xyz_deg: changes.euler ?? [...o.euler_xyz_deg],
        scale: changes.scale ?? [...o.scale],
      },
    });
    if (options.gravity !== false && this.viewerControl.gravityEnabled()) {
      this.sceneCommand.apply({ op: 'DropToFloor', args: { id: o.id } });
    }
  }
}
