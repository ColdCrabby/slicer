import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { Icon, TooltipDirective } from '@coldcrabby/ui';
import { CodeEditor } from '../code-editor/code-editor';
import { GCODE_LANGUAGE_ID } from '../code-editor/gcode-language';
import { GcodePreview } from '../../services/gcode-preview';
import { ViewerControl } from '../../services/viewer-control';

/**
 * The sliced file's own text, docked beside the scene and tied to the preview.
 *
 * The two views are one instrument: the caret follows the layer and extrusion
 * sliders, and moving the caret moves them. That is the whole point of docking
 * the text rather than offering a download — the question it answers is "what
 * does *this* move look like in the file", and a separate window cannot answer
 * it.
 *
 * Read-only for now. The file is the slicer's output, and an edit here would
 * not survive the next slice — an editable panel needs somewhere for the edit
 * to live first.
 */
@Component({
  selector: 'nexus-gcode-text-panel',
  standalone: true,
  imports: [CodeEditor, Icon, TooltipDirective],
  templateUrl: './gcode-text-panel.html',
  styleUrl: './gcode-text-panel.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class GcodeTextPanel {
  private readonly preview = inject(GcodePreview);
  private readonly viewerControl = inject(ViewerControl);

  protected readonly language = GCODE_LANGUAGE_ID;

  protected readonly text = this.preview.gcodeText;
  protected readonly loading = this.preview.loading;

  /** The line the preview is scrubbed to — what the editor marks and reveals. */
  protected readonly activeLine = this.preview.currentLine;

  /** Header readout: where in the file the preview currently is. */
  protected readonly lineSummary = computed(() => {
    const line = this.activeLine();
    return line === null ? '—' : `Line ${line.toLocaleString()}`;
  });

  protected readonly layerSummary = computed(
    () => `Layer ${this.preview.layerMax() + 1} of ${this.preview.layerCount()}`,
  );

  protected close(): void {
    this.viewerControl.setGcodeTextPanel(false);
  }

  /** Drive the preview from the caret — the text view's half of the sync. */
  protected onLineSelect(line: number): void {
    this.preview.revealLine(line);
  }
}
