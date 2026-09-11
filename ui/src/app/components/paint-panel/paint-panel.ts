import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { Icon, NumberInput, Segmented, TooltipDirective, type SegmentOption } from '@coldcrabby/ui';
import { SceneCommand } from '../../services/scene-command/scene-command';
import { SceneEngine } from '../../services/scene-engine';
import { ViewerControl, type PaintBrushMode } from '../../services/viewer-control';

const BRUSH_MODE_OPTIONS: readonly SegmentOption[] = [
  {
    value: 'enforcer',
    label: 'Enforce',
    description: 'Force support under the brush, however small the overhang',
  },
  { value: 'blocker', label: 'Block', description: 'Never generate support under the brush' },
  { value: 'erase', label: 'Erase', description: 'Remove paint under the brush' },
];

/**
 * Contextual paint-brush sub-settings for the 3D toolbar's `'paint'` object
 * mode. Mirrors the transform panel's pattern — hangs under the toolbar,
 * gated on the active object mode.
 *
 * A brush stroke itself is dispatched from `SceneSelection`'s pointer
 * handling (via `Viewer.handlePaintDab`), not from this panel — this panel
 * only owns the brush's mode/radius (read by `SceneSelection` through
 * `ViewerScene.setPaintBrush`) and the "clear all paint" action.
 */
@Component({
  selector: 'nexus-paint-panel',
  standalone: true,
  imports: [NumberInput, Segmented, Icon, TooltipDirective],
  templateUrl: './paint-panel.html',
  styleUrl: './paint-panel.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class PaintPanel {
  private readonly viewerControl = inject(ViewerControl);
  private readonly sceneEngine = inject(SceneEngine);
  private readonly sceneCommand = inject(SceneCommand);

  protected readonly brushModeOptions = BRUSH_MODE_OPTIONS;

  protected readonly visible = computed(() => this.viewerControl.objectMode() === 'paint');
  protected readonly brushMode = computed(() => this.viewerControl.paintBrushMode());
  protected readonly brushRadius = computed(() => this.viewerControl.paintBrushRadius());

  /** Painted facets across the whole plate — gates the clear button and gives a size cue. */
  protected readonly paintedFacetTotal = computed(() =>
    this.sceneEngine.objects().reduce((sum, o) => sum + o.painted_facets, 0),
  );

  protected setBrushMode(value: string): void {
    this.viewerControl.paintBrushMode.set(value as PaintBrushMode);
  }

  protected setBrushRadius(value: number): void {
    this.viewerControl.paintBrushRadius.set(value);
  }

  /** Erase every painted region on the plate, across every object at once. */
  protected clearAllPaint(): void {
    for (const object of this.sceneEngine.objects()) {
      if (object.painted_facets === 0) {
        continue;
      }
      this.sceneCommand.apply({
        op: 'SetSupportPaint',
        args: { id: object.id, encoded: null },
      });
    }
    this.sceneCommand.flush();
  }
}
