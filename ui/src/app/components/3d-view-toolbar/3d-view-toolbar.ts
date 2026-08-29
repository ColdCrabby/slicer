import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import type { ElementRef } from '@angular/core';
import { Dialog } from '../../services/dialog';
import { GcodePreview } from '../../services/gcode-preview';
import { KeyboardShortcuts } from '../../services/keyboard-shortcuts/keyboard-shortcuts';
import { NotificationService } from '../../services/notifications';
import { SceneCommand } from '../../services/scene-command/scene-command';
import { Slicer } from '../../services/slicer';
import { ViewerControl } from '../../services/viewer-control';
import { WorkplateObjects } from '../../services/workplate-objects';
import { Icon } from '../../shared/icon/icon';
import { RadioButtonValue } from '../../shared/radio-group/radio-button-value';
import { RadioGroup } from '../../shared/radio-group/radio-group';
import { TooltipDirective } from '../../shared/tooltip/tooltip.directive';
import { Card } from '../card/card';
import { OperationPipelineDialog } from '../operation-pipeline-dialog/operation-pipeline-dialog';

@Component({
  selector: 'nexus-3d-view-toolbar',
  imports: [Card, Icon, RadioGroup, RadioButtonValue, TooltipDirective],
  templateUrl: './3d-view-toolbar.html',
  styleUrl: './3d-view-toolbar.css',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ThreeDViewToolbar {
  private readonly viewerControl = inject(ViewerControl);
  private readonly slicer = inject(Slicer);
  private readonly gcodePreview = inject(GcodePreview);
  private readonly sceneCommand = inject(SceneCommand);
  private readonly dialog = inject(Dialog);
  private readonly workplate = inject(WorkplateObjects);
  private readonly notifications = inject(NotificationService);
  protected readonly keyboardShortcuts = inject(KeyboardShortcuts);

  private readonly addInput = viewChild<ElementRef<HTMLInputElement>>('addObjectInput');

  readonly selectedView = this.viewerControl.view;
  readonly selectedObjectMode = this.viewerControl.objectMode;
  readonly viewMode = this.viewerControl.viewMode;
  readonly gravityEnabled = this.viewerControl.gravityEnabled;

  /** True while models are being added, so the button can show progress. */
  protected readonly addingObjects = signal(false);

  toggleGravity(): void {
    this.gravityEnabled.update((v) => !v);
  }

  /** Auto-orient all objects in the scene. */
  autoOrient(): void {
    this.sceneCommand.autoOrient();
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
      const added = results.filter((r) => r.objectId !== undefined);
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
