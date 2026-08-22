import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import {
  FLOATS_PER_SEGMENT,
  GcodePreview,
  type GcodeViewMode,
  ROLE_LABELS,
  ROLE_ORDER,
  type RoleName,
  sampleSpeedColor,
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

  /**
   * `true` when the active channel has effectively one value across the whole
   * model (span ≈ 0). The legend then shows a single swatch instead of a
   * misleading min→max gradient.
   */
  protected readonly isUniform = computed(() => {
    if (!this.activeChannel()) {
      return false;
    }
    const { min, max } = this.preview.activeRange();
    return max > 0 && max - min <= 1e-4 * Math.max(1, Math.abs(max));
  });

  /** The single value shown when the channel is uniform. */
  protected readonly uniformLabel = computed(() => {
    const ch = this.activeChannel();
    return ch ? ch.format(this.preview.activeRange().max) : '';
  });

  /** Color of the uniform swatch (mid-ramp, matching how uniform data renders). */
  protected readonly uniformSwatchCss = `#${sampleSpeedColor(0.5).toString(16).padStart(6, '0')}`;

  /** Formatted min/max labels for the active scalar channel's legend. */
  protected readonly scalarMinLabel = computed(() => {
    const ch = this.activeChannel();
    return ch ? ch.format(this.preview.activeRange().min) : '';
  });
  protected readonly scalarMaxLabel = computed(() => {
    const ch = this.activeChannel();
    return ch ? ch.format(this.preview.activeRange().max) : '';
  });

  /**
   * Gradient position (%) of the currently-hovered extrusion, or `null` when
   * nothing is hovered or the hover belongs to a different channel.
   */
  protected readonly hoverTickPct = computed<number | null>(() => {
    const h = this.preview.hoverInfo();
    if (!h || h.channelId !== this.preview.viewMode()) {
      return null;
    }
    return h.t * 100;
  });

  /** Legend scrub readout: value label + gradient position while hovering it. */
  protected readonly scrub = signal<{ pct: number; label: string } | null>(null);
  private scrubRaf = 0;
  private pendingScrubT: number | null = null;

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

  /** Hover over the gradient: show the value at the cursor and spotlight its band. */
  protected onGradientMove(event: PointerEvent): void {
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    this.pendingScrubT = Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width));
    this.scrubRaf ||= requestAnimationFrame(() => {
      this.scrubRaf = 0;
      const t = this.pendingScrubT;
      this.pendingScrubT = null;
      if (t !== null) {
        this.applyScrub(t);
      }
    });
  }

  protected onGradientLeave(): void {
    if (this.scrubRaf) {
      cancelAnimationFrame(this.scrubRaf);
      this.scrubRaf = 0;
    }
    this.pendingScrubT = null;
    this.scrub.set(null);
    this.preview.setHoverBand(null);
  }

  private applyScrub(t: number): void {
    const ch = this.activeChannel();
    if (!ch || this.isUniform()) {
      return;
    }
    const { min, max } = this.preview.activeRange();
    const span = max - min;
    const value = min + t * span;
    const half = BAND_HALF_T * span;
    this.scrub.set({ pct: t * 100, label: ch.format(value) });
    this.preview.setHoverBand({ lo: value - half, hi: value + half });
  }
}

/** Half-width of the legend spotlight band, as a fraction of the value span. */
const BAND_HALF_T = 0.035;
