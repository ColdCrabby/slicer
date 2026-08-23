import { Component, computed, inject, signal, viewChild } from '@angular/core';
import type { ElementRef } from '@angular/core';
import { Router } from '@angular/router';
import { Viewer } from '../../components/viewer/viewer';
import { PrintArea } from '../../services/print-area';
import { Slicer } from '../../services/slicer';
import { SlicerFile } from '../../services/slicer-file';
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
  private readonly fileInputRef = viewChild.required<ElementRef<HTMLInputElement>>('fileInput');
  readonly slicerFile = inject(SlicerFile);
  private dragDepth = 0;
  readonly dragActive = signal(false);
  readonly invalidDropMessage = signal<string | null>(null);
  readonly bedLabel = computed(() => {
    const config = this.printArea.config();
    return `${config.printableAreaWidth} x ${config.printableAreaHeight} mm`;
  });
  readonly uploading = computed(() => {
    const p = this.slicerFile.uploadProgress();
    return p > 0 && p < 100;
  });

  onFileSelected(event: Event): void {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (file) {
      this.handleCandidateFile(file);
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
    const file = event.dataTransfer?.files?.[0] ?? null;
    if (file) {
      this.handleCandidateFile(file);
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

  private handleCandidateFile(file: File): void {
    if (!/\.(stl|obj|3mf)$/i.test(file.name)) {
      this.invalidDropMessage.set('Unsupported file. Use STL, OBJ, or 3MF.');
      return;
    }
    this.invalidDropMessage.set(null);
    void this.startWorkplate(file);
  }

  private async startWorkplate(file: File): Promise<void> {
    try {
      const workplate = await this.slicer.startWorkplate(file);
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
