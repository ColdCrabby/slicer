import { Component, computed, inject, signal, viewChild } from '@angular/core';
import type { ElementRef } from '@angular/core';
import { Router } from '@angular/router';
import { Viewer } from '../../components/viewer/viewer';
import { PrintArea } from '../../services/print-area';
import { Slicer } from '../../services/slicer';
import { SlicerFile } from '../../services/slicer-file';
import { WorkplateObjects } from '../../services/workplate-objects';
import { EmptyState } from '../../ui/empty-state/empty-state';

@Component({
  selector: 'nexus-slice-new',
  imports: [EmptyState, Viewer],
  templateUrl: './slice-new.component.html',
  styleUrl: './slice-new.component.scss',
})
export class SliceNew {
  private readonly router = inject(Router);
  private readonly slicer = inject(Slicer);
  private readonly printArea = inject(PrintArea);
  private readonly workplate = inject(WorkplateObjects);
  private readonly fileInputRef = viewChild.required<ElementRef<HTMLInputElement>>('fileInput');
  readonly slicerFile = inject(SlicerFile);
  private dragDepth = 0;
  readonly dragActive = signal(false);
  readonly invalidDropMessage = signal<string | null>(null);
  readonly bedLabel = computed(() => {
    const config = this.printArea.config();
    if (config.bedShape === 'circular') {
      return `Dia ${config.printableAreaWidth} mm`;
    }
    return `${config.printableAreaWidth} x ${config.printableAreaHeight} mm`;
  });
  readonly uploading = computed(() => {
    const p = this.slicerFile.uploadProgress();
    return p > 0 && p < 100;
  });

  onFileSelected(event: Event): void {
    const input = event.target as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    if (files.length > 0) {
      this.handleCandidateFiles(files);
    }
    input.value = '';
  }

  onDragEnter(event: DragEvent): void {
    event.preventDefault();
    this.dragDepth += 1;
    this.dragActive.set(true);
  }

  onDragOver(event: DragEvent): void {
    event.preventDefault();
  }

  onDragLeave(event: DragEvent): void {
    event.preventDefault();
    this.dragDepth = Math.max(0, this.dragDepth - 1);
    if (this.dragDepth === 0) {
      this.dragActive.set(false);
    }
  }

  onDrop(event: DragEvent): void {
    event.preventDefault();
    this.dragDepth = 0;
    this.dragActive.set(false);
    const files = Array.from(event.dataTransfer?.files ?? []);
    if (files.length > 0) {
      this.handleCandidateFiles(files);
    }
  }

  triggerFilePicker(event?: Event): void {
    if (this.uploading() || this.slicerFile.isPending()) {
      return;
    }
    event?.preventDefault();
    event?.stopPropagation();
    this.fileInputRef().nativeElement.click();
  }

  /**
   * Open a plate from the picked/dropped models.
   *
   * The first valid model opens the plate; the rest are queued and added by
   * the slice viewer once the scene exists.
   */
  private handleCandidateFiles(files: readonly File[]): void {
    const models = files.filter((f) => /\.(stl|obj|3mf)$/i.test(f.name));
    if (models.length === 0) {
      this.invalidDropMessage.set('Unsupported file. Use STL, OBJ, or 3MF.');
      return;
    }
    this.invalidDropMessage.set(
      models.length < files.length ? 'Some files were skipped — only STL, OBJ, and 3MF.' : null,
    );
    void this.startWorkplate(models[0], models.slice(1));
  }

  private async startWorkplate(file: File, extras: readonly File[] = []): Promise<void> {
    try {
      const workplate = await this.slicer.startWorkplate(file);
      // Queue only after the plate exists — `startWorkplate` resets the scene.
      this.workplate.queuePending(extras);
      // Carry the upload response in router state so the slice viewer can
      // pick up the `ofids` without an extra fetch. On a cold reload the
      // viewer falls back to `GET /api/request/:ruuid`.
      this.router.navigate(['/slice', workplate.requestUuid], {
        state: workplate.uploadMeta ? { uploadMeta: workplate.uploadMeta } : undefined,
      });
    } catch {
      // Error is tracked in slicerFile.uploadError
    }
  }
}
