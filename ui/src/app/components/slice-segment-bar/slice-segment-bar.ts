import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  computed,
  effect,
  inject,
  signal,
  untracked,
  viewChild,
} from '@angular/core';
import {
  FLOATS_PER_SEGMENT,
  GcodePreview,
  type GcodeViewMode,
  ROLE_GROUPS,
  ROLE_LABELS,
  type RoleGroup,
  type RoleName,
  sampleSpeedColor,
  scalarChannelFor,
  speedGradientCss,
  VIEW_MODE_LABELS,
} from '../../services/gcode-preview';
import { Select, type SelectOption, Slider } from '@coldcrabby/ui';
import { ViewerControl } from '../../services/viewer-control';
import { Viewport } from '../../services/viewport';

/** Where the user's fold preference for the inspector is remembered. */
const STORAGE_EXPANDED_KEY = 'nexus.inspector.expanded';

@Component({
  selector: 'nexus-slice-segment-bar',
  standalone: true,
  imports: [Select, Slider],
  templateUrl: './slice-segment-bar.html',
  styleUrl: './slice-segment-bar.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SliceSegmentBar {
  protected readonly preview = inject(GcodePreview);
  private readonly viewerControl = inject(ViewerControl);
  private readonly viewport = inject(Viewport);
  private readonly destroyRef = inject(DestroyRef);
  private readonly hostEl = inject(ElementRef<HTMLElement>);

  private readonly inspectorRef = viewChild<ElementRef<HTMLElement>>('inspector');

  protected readonly roleCss = this.preview.roleCss;
  protected readonly roleLabels = ROLE_LABELS;
  protected readonly roleGroups: readonly RoleGroup[] = ROLE_GROUPS;

  /**
   * Drives the card's reveal animation. Flipped true only once the preview
   * render has settled (see the constructor), so the expand transition isn't
   * fighting the heavy G-code geometry build for main-thread time (which drops
   * frames). Stays true through a reslice's brief handle-null window so the
   * inspector never collapses or re-animates mid-update.
   */
  private readonly revealSignal = signal(false);
  protected readonly revealed = this.revealSignal.asReadonly();
  private cancelReveal: (() => void) | null = null;

  /** Clamped top index for the layer slider (avoids a −¹1 max during reslice). */
  protected readonly layerMaxIndex = computed(() => Math.max(0, this.preview.layerCount() - 1));

  /**
   * Whether the legend and controls are showing, as opposed to just the header.
   *
   * The inspector is a legend, two dropdowns and two sliders stacked on top of
   * the Slice button — worth its space on a desktop, and squarely in the way
   * everywhere else. It is the tallest thing floating over the plate, and on a
   * touch device there is no cursor to move away from it, so without a fold it
   * simply stays there.
   *
   * `null` means the user has never said, in which case the room available
   * decides: open where there is a large scene and a cursor, folded on a
   * tablet, a phone or a narrow window. Once the user folds or unfolds it once,
   * that answer is theirs and is remembered across sessions and viewports.
   */
  private readonly expandedPreference = signal(this.readExpanded());

  protected readonly expanded = computed(
    () => this.expandedPreference() ?? !this.viewport.isCompact(),
  );

  /** Spoken form of the header readout, for the toggle's accessible name. */
  protected readonly layerSummary = computed(
    () => `${this.preview.layerMax() + 1} of ${this.preview.layerCount()}`,
  );

  protected toggleExpanded(): void {
    const next = !this.expanded();
    this.expandedPreference.set(next);
    this.saveExpanded(next);
  }

  private readExpanded(): boolean | null {
    try {
      const stored = localStorage.getItem(STORAGE_EXPANDED_KEY);
      return stored === null ? null : stored === 'true';
    } catch {
      return null;
    }
  }

  private saveExpanded(expanded: boolean): void {
    try {
      localStorage.setItem(STORAGE_EXPANDED_KEY, String(expanded));
    } catch {
      /* storage unavailable — the preference is simply not remembered */
    }
  }

  constructor() {
    effect(() => {
      const hasData = this.preview.gcodeHandle() !== null || this.preview.loading();
      const inGcodeView = this.viewerControl.viewMode() === 'gcode';
      untracked(() => {
        // Open only while previewing G-code with a result present; collapse
        // (animated) for model view or when there's nothing sliced yet.
        if (inGcodeView && hasData) {
          if (!this.revealSignal() && !this.cancelReveal) {
            this.scheduleReveal();
          }
          return;
        }
        this.clearReveal();
        this.revealSignal.set(false);
      });
    });

    // Keep --inspector-height locked to the inspector's true content height.
    //
    // The reveal animates `max-height: 0 → var(--inspector-height)`. Animating
    // to the *exact* pixel height (never an over-estimate) is what makes the
    // easing curve map 1:1 to the visible motion — the single technique WebKit
    // (Tauri's WKWebView / WebKitGTK) renders reliably smooth, unlike
    // `grid-template-rows` fr-interpolation. A ResizeObserver keeps the value
    // live so content that changes *while open* (the legend swapping between
    // role / scalar / uniform, or the Fan row appearing) neither clips nor
    // jumps — it resizes along the same eased transition.
    effect((onCleanup) => {
      const el = this.inspectorRef()?.nativeElement;
      if (!el) return;
      const sync = () =>
        this.hostEl.nativeElement.style.setProperty('--inspector-height', `${el.scrollHeight}px`);
      sync();
      const ro = new ResizeObserver(sync);
      ro.observe(el);
      onCleanup(() => ro.disconnect());
    });

    this.destroyRef.onDestroy(() => this.clearReveal());
  }

  /**
   * Open the inspector only after the main thread goes idle (the G-code build
   * and first paint have finished), then start the transition on a fresh frame.
   */
  private scheduleReveal(): void {
    const open = () => {
      this.cancelReveal = null;
      // --inspector-height is kept current by the ResizeObserver in the
      // constructor, so the max-height reveal already has an exact target.
      this.revealSignal.set(true);
    };
    const win = window as unknown as {
      requestIdleCallback?: (cb: () => void, opts?: { timeout?: number }) => number;
      cancelIdleCallback?: (id: number) => void;
    };
    if (typeof win.requestIdleCallback === 'function') {
      let rafId = 0;
      const idleId = win.requestIdleCallback(
        () => {
          rafId = requestAnimationFrame(open);
        },
        { timeout: 500 },
      );
      this.cancelReveal = () => {
        win.cancelIdleCallback?.(idleId);
        if (rafId) cancelAnimationFrame(rafId);
      };
      return;
    }
    // WebKit fallback: two frames past the synchronous build + a short settle.
    let raf1 = 0;
    let raf2 = 0;
    let timer = 0;
    raf1 = requestAnimationFrame(() => {
      raf2 = requestAnimationFrame(() => {
        timer = window.setTimeout(open, 80);
      });
    });
    this.cancelReveal = () => {
      cancelAnimationFrame(raf1);
      cancelAnimationFrame(raf2);
      window.clearTimeout(timer);
    };
  }

  private clearReveal(): void {
    this.cancelReveal?.();
    this.cancelReveal = null;
  }

  /** View-mode dropdown options (filtered to what the model actually has). */
  protected readonly viewModes = this.preview.availableViewModes;
  protected readonly viewModeLabels = VIEW_MODE_LABELS;

  /** `nexus-select` options for the "Color by" dropdown. */
  protected readonly viewModeOptions = computed<SelectOption[]>(() =>
    this.viewModes().map((m) => ({ value: m, label: this.viewModeLabels[m] })),
  );

  /** Fans discovered in the model, for the secondary fan selector. */
  protected readonly fans = this.preview.discoveredFans;

  /** `nexus-select` options for the fan sub-selector. */
  protected readonly fanOptions = computed<SelectOption[]>(() =>
    this.fans().map((f) => ({ value: f.key, label: f.label })),
  );

  /** Show the fan sub-selector only in fan mode with more than one fan. */
  protected readonly showFanSelector = computed(
    () => this.preview.effectiveViewMode() === 'fan' && this.preview.discoveredFans().length > 1,
  );

  /** Static CSS gradient mirroring the scalar color ramp (low → high). */
  protected readonly scalarGradient = speedGradientCss();

  /** Descriptor of the active scalar channel, or `null` in category mode. */
  protected readonly activeChannel = computed(() =>
    scalarChannelFor(this.preview.effectiveViewMode()),
  );

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
    if (!h || h.channelId !== this.preview.effectiveViewMode()) {
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

  // ── Event handlers ───────────────────────────────────────────────────────

  /** Layer navigation (top of the inspector). */
  protected setLayer(value: number): void {
    this.preview.setLayerMax(value);
  }

  protected onWheelLayer(event: WheelEvent): void {
    event.preventDefault();
    const step = event.deltaY < 0 ? 1 : -1;
    this.preview.setLayerMax(this.preview.layerMax() + step);
  }

  protected toggleShowAll(): void {
    this.preview.toggleShowAllLayers();
  }

  /** Segment scrub inside the active top layer. */
  protected onSegmentValue(value: number): void {
    const total = this.layerSegmentCount();
    this.preview.setSegmentProgress(total > 0 ? value / total : 1);
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

  protected onModeValue(value: string): void {
    this.preview.setViewMode(value as GcodeViewMode);
  }

  protected onFanValue(value: string): void {
    this.preview.setSelectedFan(value);
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
