import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import type { ElementRef } from '@angular/core';
import { Arrange } from '../../services/arrange';
import { Dialog } from '../../services/dialog';
import { GcodePreview } from '../../services/gcode-preview';
import { HistoryControlsPreference } from '../../services/history-controls-preference';
import { KeyboardShortcuts } from '../../services/keyboard-shortcuts/keyboard-shortcuts';
import { NotificationService } from '../../services/notifications';
import { SceneHistory } from '../../services/scene-history/scene-history';
import { Slicer } from '../../services/slicer';
import { ViewerControl } from '../../services/viewer-control';
import { WorkplateObjects } from '../../services/workplate-objects';
import {
  Icon,
  RadioButtonValue,
  RadioGroupDirective as RadioGroup,
  TooltipDirective,
} from '@coldcrabby/ui';
import { Card } from '../card/card';
import { OperationPipelineDialog } from '../operation-pipeline-dialog/operation-pipeline-dialog';
import { PlacementPanel } from '../placement-panel/placement-panel';
import { TransformPanel } from '../transform-panel/transform-panel';

@Component({
  selector: 'nexus-3d-view-toolbar',
  imports: [
    Card,
    Icon,
    RadioGroup,
    RadioButtonValue,
    TooltipDirective,
    TransformPanel,
    PlacementPanel,
  ],
  templateUrl: './3d-view-toolbar.html',
  styleUrl: './3d-view-toolbar.css',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ThreeDViewToolbar {
  private readonly viewerControl = inject(ViewerControl);
  private readonly slicer = inject(Slicer);
  private readonly gcodePreview = inject(GcodePreview);
  private readonly dialog = inject(Dialog);
  private readonly workplate = inject(WorkplateObjects);
  private readonly notifications = inject(NotificationService);
  private readonly arrange = inject(Arrange);
  private readonly history = inject(SceneHistory);
  protected readonly historyControls = inject(HistoryControlsPreference);
  protected readonly keyboardShortcuts = inject(KeyboardShortcuts);

  private readonly addInput = viewChild<ElementRef<HTMLInputElement>>('addObjectInput');

  readonly selectedView = this.viewerControl.view;
  readonly selectedObjectMode = this.viewerControl.objectMode;
  readonly viewMode = this.viewerControl.viewMode;
  readonly gravityEnabled = this.viewerControl.gravityEnabled;

  /**
   * True while the user is working on the plate rather than inspecting a
   * slice. Plate-editing controls are hidden in G-code preview: the view shows
   * toolpaths, so moving or adding a model there changes something the user
   * cannot see — the same reason the objects list hides.
   */
  protected readonly editingPlate = computed(() => this.viewMode() === 'model');

  /**
   * Whether to show the on-canvas undo/redo buttons. They exist for
   * keyboard-less devices where the ⌘/Ctrl+Z shortcut is unreachable, so they
   * only appear while editing the plate and when the history preference allows.
   */
  protected readonly showHistoryButtons = computed(
    () => this.editingPlate() && this.historyControls.visible(),
  );

  /** Whether stepping back/forward through scene history is possible. */
  protected readonly canUndo = this.history.canUndo;
  protected readonly canRedo = this.history.canRedo;

  protected undo(): void {
    this.history.undo();
  }

  protected redo(): void {
    this.history.redo();
  }

  /** True while models are being added, so the button can show progress. */
  protected readonly addingObjects = signal(false);

  /** Whether the placement tool's sub-settings card is showing. */
  protected readonly placementOpen = this.arrange.optionsOpen;

  /** One-line recap of what placing will do, for the button's tooltip. */
  protected readonly placementSummary = computed(() => {
    const { autoOrient, spacingMm, preferredOrientationDeg } = this.arrange.settings();
    const parts = [autoOrient ? 'auto-orient on' : 'auto-orient off', `${spacingMm} mm gap`];
    if (autoOrient && preferredOrientationDeg !== 0) {
      parts.push(`${preferredOrientationDeg}° preferred angle`);
    }
    return parts.join(' · ');
  });

  protected togglePlacement(): void {
    this.arrange.toggleOptions();
  }

  toggleGravity(): void {
    this.gravityEnabled.update((v) => !v);
  }

  /** Open the file picker to place more models on the current plate. */
  protected promptAddObjects(): void {
    this.addInput()?.nativeElement.click();
  }

  /**
   * Add every picked model to the plate without disturbing what is already
   * there. Each file is reported individually so one unreadable model does not
   * hide the ones that loaded.
   */
  protected async onAddObjects(event: Event): Promise<void> {
    const input = event.target as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    // Reset immediately so picking the same file twice still fires a change.
    input.value = '';
    if (files.length === 0) {
      return;
    }

    this.addingObjects.set(true);
    try {
      const results = await this.workplate.addFiles(files);
      const added = results.filter((r) => r.objectIds !== undefined);
      const failed = results.filter((r) => r.error);

      if (added.length > 0) {
        this.notifications.success(
          added.length === 1 ? 'Model added' : `${added.length} models added`,
          added.map((r) => r.file.name).join(', '),
        );
      }
      for (const failure of failed) {
        this.notifications.error(`Could not add ${failure.file.name}`, failure.error);
      }
    } finally {
      this.addingObjects.set(false);
    }
  }

  /** Toggle between perspective and orthographic projection. */
  toggleProjection(): void {
    this.selectedView.update((v) => (v === 'perspective' ? 'ortho' : 'perspective'));
  }

  /** True once a slice result is available (either loading or fully parsed). */
  protected readonly hasSliceResult = computed(
    () => this.gcodePreview.gcodeHandle() !== null || this.gcodePreview.loading(),
  );

  resetView(): void {
    this.viewerControl.reset();
  }

  /** Open the operation-pipeline inspector dialog. */
  showOperationPipeline(): void {
    this.dialog
      .alert({
        title: 'Operation pipeline',
        content: OperationPipelineDialog,
        confirmLabel: 'Close',
        preferredWidth: '860px',
      })
      .subscribe();
  }

  toggleViewMode(): void {
    if (this.viewMode() === 'gcode') {
      this.viewerControl.viewMode.set('model');
      return;
    }

    this.viewerControl.viewMode.set('gcode');

    // If no slice exists yet and we're not already slicing, kick one off.
    const status = this.slicer.status();
    if (!this.gcodePreview.gcodeHandle() && status !== 'slicing' && status !== 'uploading') {
      void this.slicer.slice();
    }
  }
}
