import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { GcodePreview } from '../../services/gcode-preview';

@Component({
  selector: 'nexus-slice-layer-bar',
  standalone: true,
  templateUrl: './slice-layer-bar.html',
  styleUrl: './slice-layer-bar.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SliceLayerBar {
  protected readonly preview = inject(GcodePreview);

  /** Z height of the currently selected top layer. */
  protected readonly currentZ = computed(() => {
    const handle = this.preview.gcodeHandle();
    if (!handle) {
      return null;
    }
    return handle.layerZ(this.preview.layerMax());
  });

  /**
   * Percentage of the custom track fill, measured from the bottom.
   * 0 % = layer 0 selected; 100 % = topmost layer selected.
   */
  protected readonly fillPercent = computed(() => {
    const count = this.preview.layerCount();
    if (count <= 1) {
      return 100;
    }
    return (this.preview.layerMax() / (count - 1)) * 100;
  });

  // ── Event handlers ───────────────────────────────────────────────────────

  protected onInput(event: Event): void {
    const raw = parseInt((event.target as HTMLInputElement).value, 10);
    this.preview.setLayerMax(raw);
  }

  protected onWheel(event: WheelEvent): void {
    event.preventDefault();
    // Scroll up (deltaY < 0) → higher layer
    const step = event.deltaY < 0 ? 1 : -1;
    this.preview.setLayerMax(this.preview.layerMax() + step);
  }

  protected toggleMode(): void {
    this.preview.toggleShowAllLayers();
  }
}
