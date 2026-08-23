import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { Router } from '@angular/router';
import { NexusSlicingShell } from '../../nexus/layout/slicing-shell/slicing-shell';
import { GcodePreview } from '../../services/gcode-preview';
import { KeyboardShortcuts } from '../../services/keyboard-shortcuts/keyboard-shortcuts';
import { SceneCommand } from '../../services/scene-command/scene-command';
import { Slicer } from '../../services/slicer';
import { ViewerControl } from '../../services/viewer-control';
import { Icon } from '../../shared/icon/icon';
import { RadioButtonValue } from '../../shared/radio-group/radio-button-value';
import { RadioGroup } from '../../shared/radio-group/radio-group';
import { TooltipDirective } from '../../shared/tooltip/tooltip.directive';
import { Card } from '../card/card';

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
  private readonly router = inject(Router);
  protected readonly shell = inject(NexusSlicingShell);
  protected readonly keyboardShortcuts = inject(KeyboardShortcuts);

  readonly selectedView = this.viewerControl.view;
  readonly selectedObjectMode = this.viewerControl.objectMode;
  readonly viewMode = this.viewerControl.viewMode;
  readonly gravityEnabled = this.viewerControl.gravityEnabled;

  toggleGravity(): void {
    this.gravityEnabled.update((v) => !v);
  }

  /** Auto-orient all objects in the scene. */
  autoOrient(): void {
    this.sceneCommand.autoOrient();
  }

  /** True once a slice result is available (either loading or fully parsed). */
  protected get hasSliceResult(): boolean {
    return this.gcodePreview.gcodeHandle() !== null || this.gcodePreview.loading();
  }

  /** True when a file is loaded and eligible to slice. */
  protected get canSlice(): boolean {
    return this.slicer.selectedFile() !== null;
  }

  resetView(): void {
    this.viewerControl.reset();
  }

  showKeyboardShortcuts(): void {
    this.router.navigate(['/settings/shortcuts']);
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
