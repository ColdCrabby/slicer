import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { SceneEngine } from '../../services/scene-engine';
import { Slicer } from '../../services/slicer';
import { Icon } from '../../shared/icon/icon';
import { TooltipDirective } from '../../shared/tooltip/tooltip.directive';
import { CodeEditor } from '../code-editor/code-editor';

type PipelineSection = 'scene' | 'settings';

/**
 * Body of the "operation pipeline" dialog: the exact scene snapshot and slice
 * settings payloads that are sent over WebSocket when a slice job starts,
 * shown side-by-side in read-only Monaco editors with a per-section copy action.
 *
 * Rendered by the {@link Dialog} service via `NgComponentOutlet`, so it pulls
 * its data straight from the injected services rather than through inputs.
 */
@Component({
  selector: 'nexus-operation-pipeline-dialog',
  standalone: true,
  imports: [CodeEditor, Icon, TooltipDirective],
  templateUrl: './operation-pipeline-dialog.html',
  styleUrl: './operation-pipeline-dialog.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class OperationPipelineDialog {
  private readonly sceneEngine = inject(SceneEngine);
  private readonly slicer = inject(Slicer);

  /** Scene snapshot serialised as JSON (`bigint` ids stringified so it doesn't throw). */
  protected readonly snapshotJson = computed(() =>
    JSON.stringify(
      this.sceneEngine.snapshot(),
      (_key, value) => (typeof value === 'bigint' ? String(value) : value),
      2,
    ),
  );

  /** Resolved slice settings serialised as JSON. */
  protected readonly sliceParamsJson = computed(() =>
    JSON.stringify(this.slicer.settings(), null, 2),
  );

  /** Which section was most recently copied — drives the transient check-icon feedback. */
  protected readonly copied = signal<PipelineSection | null>(null);
  private copyTimer: ReturnType<typeof setTimeout> | null = null;

  protected async copy(text: string, which: PipelineSection): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      this.copied.set(which);
      if (this.copyTimer) {
        clearTimeout(this.copyTimer);
      }
      this.copyTimer = setTimeout(() => this.copied.set(null), 1500);
    } catch {
      // Clipboard access denied — silently ignore.
    }
  }
}
