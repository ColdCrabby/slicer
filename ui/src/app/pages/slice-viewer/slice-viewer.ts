import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  signal,
  untracked,
} from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { ActivatedRoute } from '@angular/router';
import { map } from 'rxjs';
import { Viewer } from '../../components/viewer';
import { NotificationService } from '../../services/notifications';
import { Slicer } from '../../services/slicer';
import { SlicerFile } from '../../services/slicer-file';
import { ViewerControl } from '../../services/viewer-control';
import { WorkplateSession } from '../../services/workplate-session';
import { WorkplateObjects } from '../../services/workplate-objects';
import { Icon } from '@coldcrabby/ui';

@Component({
  selector: 'nexus-slice-viewer',
  standalone: true,
  imports: [Viewer, Icon],
  templateUrl: './slice-viewer.component.html',
  styleUrl: './slice-viewer.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SliceViewer {
  readonly #activatedRoute = inject(ActivatedRoute);
  readonly #slicer = inject(Slicer);
  readonly #slicerFile = inject(SlicerFile);
  readonly #notifications = inject(NotificationService);
  readonly #viewerControl = inject(ViewerControl);
  readonly #session = inject(WorkplateSession);
  readonly #workplate = inject(WorkplateObjects);

  readonly requestUuid = toSignal(
    this.#activatedRoute.params.pipe(map((params) => params['requestUuid'] as string | undefined)),
  );

  /** The user-selected STL (when available) is shown in model mode. */
  readonly modelFile = this.#slicerFile.selectedFile;

  /**
   * Upload id backing {@link modelFile}, so the object the viewer creates for
   * it can be resolved back to the right bytes at slice time.
   */
  readonly modelSourceId = computed(() => this.#slicerFile.files()[0]?.fileId ?? null);

  /** Driven by the toolbar toggle; auto-advances to 'gcode' when a slice completes. */
  readonly viewerMode = this.#viewerControl.viewMode;

  /** True while the plate on screen is being rebuilt from what it remembers. */
  readonly restoring = this.#session.restoring;

  /** Highlight the viewport while a file drag is over it. */
  readonly dragActive = signal(false);
  /** Nested dragenter/dragleave pairs, so leaving a child doesn't clear the state. */
  #dragDepth = 0;

  onDragEnter(event: DragEvent): void {
    if (!this.#dragHasFiles(event)) {
      return;
    }
    event.preventDefault();
    this.#dragDepth += 1;
    this.dragActive.set(true);
  }

  onDragOver(event: DragEvent): void {
    if (!this.#dragHasFiles(event)) {
      return;
    }
    // Required for the drop to fire at all.
    event.preventDefault();
  }

  onDragLeave(event: DragEvent): void {
    if (!this.#dragHasFiles(event)) {
      return;
    }
    event.preventDefault();
    this.#dragDepth = Math.max(0, this.#dragDepth - 1);
    if (this.#dragDepth === 0) {
      this.dragActive.set(false);
    }
  }

  /**
   * Drop models onto the open plate to add them.
   *
   * Dropping here *adds* — it never replaces the plate. Starting a fresh plate
   * is what the home screen's drop zone is for.
   */
  onDrop(event: DragEvent): void {
    event.preventDefault();
    this.#dragDepth = 0;
    this.dragActive.set(false);
    const files = Array.from(event.dataTransfer?.files ?? []);
    if (files.length > 0) {
      void this.#workplate.addFilesWithFeedback(files);
    }
  }

  /** Whether the current drag carries files (ignore text/element drags). */
  #dragHasFiles(event: DragEvent): boolean {
    return Array.from(event.dataTransfer?.types ?? []).includes('Files');
  }

  /**
   * Add any models queued while the plate was being opened.
   *
   * Deliberately driven by the viewer's `loadComplete` rather than the route
   * effect: opening a plate swaps the viewer's `model` input, and that swap
   * tears the scene down. Adding earlier would have the teardown delete the
   * very objects that were just added.
   */
  onViewerLoadComplete(event: { mode: string }): void {
    if (event.mode !== 'model' || this.#workplate.pendingCount() === 0) {
      return;
    }
    void this.#flushQueuedModels();
  }

  async #flushQueuedModels(): Promise<void> {
    const results = await this.#workplate.flushPending();
    const added = results.filter((r) => r.objectIds !== undefined);
    if (added.length > 0) {
      this.#notifications.success(
        added.length === 1 ? 'Model added' : `${added.length} models added`,
        added.map((r) => r.file.name).join(', '),
      );
    }
    for (const failure of results.filter((r) => r.error)) {
      this.#notifications.error(`Could not add ${failure.file.name}`, failure.error);
    }
  }

  constructor() {
    // Someone else changed this plate on a shared engine.
    //
    // A prompt rather than a reload: two people on one plate is ordinary, and
    // taking the scene away from whoever typed second is worse than letting
    // them choose the moment. It goes through the scene strip like everything
    // else the plate has to say — as its own floating card it claimed the
    // top-centre band the view toolbar already owns, and read as a different
    // kind of object than the notices appearing a few pixels below it.
    //
    // Dismissing it *is* the second answer ("keep mine"), which is why it
    // needs only one button.
    effect(() => {
      const change = this.#session.changedElsewhere();
      if (!change) {
        return;
      }
      untracked(() =>
        this.#notifications.prompt(
          'This plate was changed elsewhere',
          { label: 'Reload plate', run: () => void this.#session.refresh(change.uuid) },
          {
            icon: 'cloud',
            message: 'Or keep working on your version.',
            onDismiss: () => this.#session.keepMine(),
          },
        ),
      );
    });

    // Follow a finished slice into G-code preview, as far as the preference
    // allows. `auto` follows a slice the user pressed and leaves an automatic
    // re-slice alone: the plate-editing tools are hidden in preview, so being
    // pulled there mid-edit lands the next drag on a view that cannot show it.
    // Never switches *away* from preview, so someone already inspecting a slice
    // stays put whatever re-sliced it.
    //
    // Only `status` is tracked. Reading the preference reactively would make
    // changing it in Settings retro-apply to the slice already on screen and
    // yank the user into preview; it should decide what the *next* slice does.
    effect(() => {
      if (this.#slicer.status() !== 'done') {
        return;
      }
      untracked(() => {
        const follow = this.#viewerControl.previewFollow();
        if (follow === 'never' || (follow === 'auto' && this.#slicer.sliceWasAutomatic())) {
          return;
        }
        this.#viewerControl.viewMode.set('gcode');
      });
    });

    // Every route change is a plate change, and a plate change is one
    // operation that belongs to one owner. `WorkplateSession` is that owner:
    // it tears the old plate down, restores the new one's files, objects,
    // placements and presets, and does so identically in all four runtimes.
    // Asking for the plate already on screen costs nothing, so the effect can
    // simply state the intent on every emission.
    effect(() => {
      const uuid = this.requestUuid();
      if (uuid) {
        untracked(() => void this.#session.open(uuid));
      }
    });
  }
}
