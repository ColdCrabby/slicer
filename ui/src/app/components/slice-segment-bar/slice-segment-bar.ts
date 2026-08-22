import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import {
  FLOATS_PER_SEGMENT,
  GcodePreview,
  type GcodeViewMode,
  ROLE_LABELS,
  ROLE_ORDER,
  type RoleName,
  scalarChannelFor,
  speedGradientCss,
  VIEW_MODE_LABELS,
} from '../../services/gcode-preview';

@Component({
  selector: 'nexus-slice-segment-bar',
  standalone: true,
  imports: [],
  templateUrl: './slice-segment-bar.html',
  styleUrl: './slice-segment-bar.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SliceSegmentBar {
  protected readonly preview = inject(GcodePreview);

  protected readonly roleCss = this.preview.roleCss;
  protected readonly roleLabels = ROLE_LABELS;
  protected readonly roleOrder: readonly RoleName[] = ROLE_ORDER;

  /** View-mode dropdown options (filtered to what the model actually has). */
  protected readonly viewModes = this.preview.availableViewModes;
  protected readonly viewModeLabels = VIEW_MODE_LABELS;

  /** Fans discovered in the model, for the secondary fan selector. */
  protected readonly fans = this.preview.discoveredFans;

  /** Show the fan sub-selector only in fan mode with more than one fan. */
  protected readonly showFanSelector = computed(
    () => this.preview.viewMode() === 'fan' && this.preview.discoveredFans().length > 1,
  );

  /** Static CSS gradient mirroring the scalar color ramp (low → high). */
  protected readonly scalarGradient = speedGradientCss();

  /** Descriptor of the active scalar channel, or `null` in category mode. */
  protected readonly activeChannel = computed(() => scalarChannelFor(this.preview.viewMode()));

  /** Formatted min/max labels for the active scalar channel's legend. */
  protected readonly scalarMinLabel = computed(() => {
    const ch = this.activeChannel();
    return ch ? ch.format(this.preview.activeRange().min) : '';
  });
  protected readonly scalarMaxLabel = computed(() => {
    const ch = this.activeChannel();
    return ch ? ch.format(this.preview.activeRange().max) : '';
  });

  /** Total move segments in the current top layer derived from its geometry buffers. */
  protected readonly layerSegmentCount = computed(() => {
    const handle = this.preview.gcodeHandle();
    if (!handle) {
      return 0;
    }
    const layer = handle.getLayer(this.preview.layerMax());
    let totalFloats = 0;
    const blocksCount = layer.blocksCount();
    for (let i = 0; i < blocksCount; i++) {
      totalFloats += layer.blockData(i).length;
    }
    return totalFloats / FLOATS_PER_SEGMENT;
  });

  /** Segment slider integer value derived from the fractional signal and real segment count. */
  protected readonly segmentSliderValue = computed(() =>
    Math.round(this.preview.segmentProgress() * this.layerSegmentCount()),
  );

  /** CSS `right%` for the scrub track fill (from left edge to thumb). */
  protected readonly scrubFillRight = computed(() => {
    const total = this.layerSegmentCount();
    if (total === 0) {
      return 0;
    }
    return (1 - this.segmentSliderValue() / total) * 100;
  });

  // ── Event handlers ───────────────────────────────────────────────────────

  protected onSegmentInput(event: Event): void {
    const raw = parseInt((event.target as HTMLInputElement).value, 10);
    const total = this.layerSegmentCount();
    this.preview.setSegmentProgress(total > 0 ? raw / total : 1);
  }

  protected onWheelSegment(event: WheelEvent): void {
    event.preventDefault();
    const total = this.layerSegmentCount();
    if (total === 0) {
      return;
    }
    const step = event.deltaY < 0 ? 1 : -1;
    const current = Math.round(this.preview.segmentProgress() * total);
    this.preview.setSegmentProgress((current + step) / total);
  }

  protected toggleRole(role: RoleName): void {
    this.preview.toggleRole(role);
  }

  protected onModeChange(event: Event): void {
    this.preview.setViewMode((event.target as HTMLSelectElement).value as GcodeViewMode);
  }

  protected onFanChange(event: Event): void {
    this.preview.setSelectedFan((event.target as HTMLSelectElement).value);
  }
}
