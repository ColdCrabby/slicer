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
 * Contextual transform sub-settings for the selected object.
 *
 * Sits directly beneath the 3D toolbar's object-mode selector and mirrors the
 * OrcaSlicer transform toolbar: whichever manipulation mode is active
 * (translate / rotate / scale) reveals precise numeric fields for that
 * operation on the single selected object. Values are read live from the WASM
 * scene engine snapshot (so gizmo drags update the fields), and every edit is
 * dispatched as an absolute `SetTransform` op through {@link SceneCommand} so
 * it participates in undo/redo exactly like a gizmo gesture.
 *
 * The panel renders nothing unless exactly one object is selected and the
 * active object mode is a transform mode.
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

  /** The single selected object, or null when the selection is not exactly one. */
  protected readonly selected = computed<SceneObjectSnapshot | null>(() => {
    const ids = this.viewerControl.selectedObjectIds();
    if (ids.length !== 1) {
      return null;
    }
    const id = ids[0];
    return this.sceneEngine.objects().find((o) => o.id === id) ?? null;
  });

  /** Whether the panel should be shown at all. */
  protected readonly visible = computed(() => this.mode() !== null && this.selected() !== null);

  /** Which unit the scale mode edits (percent of original vs. absolute mm). */
  protected readonly scaleUnit = signal<ScaleUnit>('percent');

  protected setScaleUnit(value: string): void {
    this.scaleUnit.set(value as ScaleUnit);
  }

  /** When on, editing one scale/size axis scales the others proportionally. */
  protected readonly uniform = signal(true);

  /** Live position (mm) of the selection. */
  protected readonly position = computed(() => round3(this.selected()?.translation ?? [0, 0, 0]));

  /** Live Euler-XYZ rotation (degrees) of the selection. */
  protected readonly rotation = computed(() =>
    round3(this.selected()?.euler_xyz_deg ?? [0, 0, 0], 1),
  );

  /** Live per-axis scale expressed as a percentage of the original mesh. */
  protected readonly scalePercent = computed<[number, number, number]>(() => {
    const s = this.selected()?.scale ?? [1, 1, 1];
    return round3([s[0] * 100, s[1] * 100, s[2] * 100], 1);
  });

  /** Live world-space bounding-box size (mm) of the selection. */
  protected readonly size = computed<[number, number, number]>(() => {
    const aabb = this.selected()?.world_aabb;
    if (!aabb) {
      return [0, 0, 0];
    }
    const [min, max] = aabb;
    return round3([max[0] - min[0], max[1] - min[1], max[2] - min[2]]);
  });

  // ---------------------------------------------------------------------------
  // Position
  // ---------------------------------------------------------------------------

  protected setPosition(axis: Axis, value: number): void {
    const o = this.selected();
    if (!o) {
      return;
    }
    const translation: [number, number, number] = [...o.translation];
    translation[axis] = value;
    // Editing Z is an explicit vertical placement — never let gravity undo it.
    this.commitTransform(o, { translation }, { gravity: axis !== 2 });
  }

  /** Centre the selection on the bed (X/Y), keeping Z. */
  protected centerOnBed(): void {
    const o = this.selected();
    if (!o) {
      return;
    }
    this.sceneCommand.apply({ op: 'CenterOnBed', args: { id: o.id } });
  }

  /** Drop the selection so its lowest point rests on the bed. */
  protected dropToFloor(): void {
    const o = this.selected();
    if (!o) {
      return;
    }
    this.sceneCommand.apply({ op: 'DropToFloor', args: { id: o.id } });
  }

  // ---------------------------------------------------------------------------
  // Rotation
  // ---------------------------------------------------------------------------

  protected setRotation(axis: Axis, value: number): void {
    const o = this.selected();
    if (!o) {
      return;
    }
    const euler: [number, number, number] = [...o.euler_xyz_deg];
    euler[axis] = value;
    this.commitTransform(o, { euler });
  }

  /** Reset all rotation to zero. */
  protected resetRotation(): void {
    const o = this.selected();
    if (!o) {
      return;
    }
    this.commitTransform(o, { euler: [0, 0, 0] });
  }

  // ---------------------------------------------------------------------------
  // Scale
  // ---------------------------------------------------------------------------

  protected setScalePercent(axis: Axis, value: number): void {
    const o = this.selected();
    if (!o) {
      return;
    }
    const target = value / 100;
    this.applyScaleAxis(o, axis, target);
  }

  protected setSize(axis: Axis, value: number): void {
    const o = this.selected();
    if (!o) {
      return;
    }
    const currentSize = this.size();
    const cur = currentSize[axis];
    if (cur <= 0) {
      return;
    }
    // Convert the target dimension into an absolute per-axis scale factor.
    const target = (o.scale[axis] * value) / cur;
    this.applyScaleAxis(o, axis, target);
  }

  /** Reset scale on every axis to 100%. */
  protected resetScale(): void {
    const o = this.selected();
    if (!o) {
      return;
    }
    this.commitTransform(o, { scale: [1, 1, 1] });
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
