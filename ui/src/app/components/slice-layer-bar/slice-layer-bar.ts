import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { GcodePreview } from '../../services/gcode-preview';
import { Slicer } from '../../services/slicer';

@Component({
  selector: 'nexus-slice-layer-bar',
  standalone: true,
  templateUrl: './slice-layer-bar.html',
  styleUrl: './slice-layer-bar.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SliceLayerBar {
  protected readonly preview = inject(GcodePreview);
  private readonly slicer = inject(Slicer);

  /**
   * Percentage positions (bottom-up, matching `fillPercent`) of every layer
   * carrying a pause/color-change/custom trigger (issue #113).
   */
  protected readonly triggerMarkers = computed(() => {
    const count = this.preview.layerCount();
    if (count <= 1) {
      return [] as { layerIndex: number; percent: number }[];
    }
    return this.preview
      .triggerLayers()
      .map((layerIndex) => ({ layerIndex, percent: (layerIndex / (count - 1)) * 100 }));
  });

  /** `true` when the currently selected layer already has a trigger. */
  protected readonly currentLayerHasTrigger = computed(() =>
    this.preview.triggerLayers().includes(this.preview.layerMax()),
  );

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

  /** Add a pause/color-change trigger at the currently selected layer. */
  protected addTrigger(action: 'pause' | 'color_change'): void {
    const layer = this.preview.layerMax() + 1;
    if (action === 'pause') {
      this.slicer.addLayerTrigger(layer, { type: 'pause' });
    } else {
      this.slicer.addLayerTrigger(layer, { type: 'color_change' });
    }
  }

  /** Remove every trigger anchored to the currently selected layer. */
  protected removeCurrentTrigger(): void {
    this.slicer.removeLayerTrigger(this.preview.layerMax() + 1);
  }

  /** Jump to the layer a marker sits on, so it can be inspected or removed. */
  protected selectTriggerLayer(layerIndex: number): void {
    this.preview.setLayerMax(layerIndex);
  }
}
