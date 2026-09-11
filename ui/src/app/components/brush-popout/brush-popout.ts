import {
  ChangeDetectionStrategy,
  Component,
  computed,
  ElementRef,
  HostListener,
  inject,
} from '@angular/core';
import { Segmented, Slider, type SegmentOption } from '@coldcrabby/ui';
import {
  PAINT_RADIUS_MAX_MM,
  PAINT_RADIUS_MIN_MM,
  ViewerControl,
  type PaintBrushMode,
} from '../../services/viewer-control';

const BRUSH_MODE_OPTIONS: readonly SegmentOption[] = [
  { value: 'enforcer', label: 'Enforce' },
  { value: 'blocker', label: 'Block' },
  { value: 'erase', label: 'Erase' },
];

/** Half the card's width, used to centre it on the summoning pointer. */
const CARD_HALF_WIDTH_PX = 108;
/** Gap between the pointer and the card's top edge. */
const POINTER_OFFSET_PX = 16;

/**
 * Quick-adjust brush controls, summoned at the pointer.
 *
 * The paint panel under the toolbar holds the same two settings, but reaching
 * it means leaving the model between strokes. This opens them where the hand
 * already is — the same reason an image editor puts brush size on a popup
 * rather than only in a docked panel.
 */
@Component({
  selector: 'nexus-brush-popout',
  standalone: true,
  imports: [Segmented, Slider],
  changeDetection: ChangeDetectionStrategy.OnPush,
  styleUrl: './brush-popout.scss',
  template: `
    @if (position(); as at) {
      <div
        class="brush-popout"
        role="dialog"
        aria-label="Brush settings"
        [style.left.px]="at.x"
        [style.top.px]="at.y"
      >
        <nexus-segmented
          [options]="modeOptions"
          [value]="mode()"
          (valueChange)="setMode($event)"
          label="Brush mode"
        />
        <nexus-slider
          [value]="radius()"
          [min]="minRadius"
          [max]="maxRadius"
          [step]="0.1"
          unit="mm"
          label="Brush size"
          (valueChange)="setRadius($event)"
        />
        <p class="bp-hint">Scroll to resize · Esc to close</p>
      </div>
    }
  `,
})
export class BrushPopout {
  private readonly viewerControl = inject(ViewerControl);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  protected readonly modeOptions = BRUSH_MODE_OPTIONS;
  protected readonly minRadius = PAINT_RADIUS_MIN_MM;
  protected readonly maxRadius = PAINT_RADIUS_MAX_MM;

  protected readonly mode = computed(() => this.viewerControl.paintBrushMode());
  protected readonly radius = computed(() => this.viewerControl.paintBrushRadius());

  /**
   * Where to draw the card, clamped so a summon near an edge still lands fully
   * on screen rather than half off it.
   */
  protected readonly position = computed(() => {
    const at = this.viewerControl.brushPopoutAt();
    if (!at || this.viewerControl.objectMode() !== 'paint') {
      return null;
    }
    const maxX = window.innerWidth - CARD_HALF_WIDTH_PX;
    return {
      x: Math.min(Math.max(at.x, CARD_HALF_WIDTH_PX), Math.max(maxX, CARD_HALF_WIDTH_PX)),
      y: at.y + POINTER_OFFSET_PX,
    };
  });

  protected setMode(value: string): void {
    this.viewerControl.paintBrushMode.set(value as PaintBrushMode);
  }

  protected setRadius(value: number): void {
    this.viewerControl.paintBrushRadius.set(value);
  }

  @HostListener('document:keydown.escape')
  protected close(): void {
    this.viewerControl.brushPopoutAt.set(null);
  }

  /**
   * A press anywhere but the card dismisses it — including on the canvas, where
   * the alternative is a card sitting over the model the user is trying to
   * paint. Bound on `pointerdown` so it closes before the stroke starts.
   */
  @HostListener('document:pointerdown', ['$event'])
  protected onDocumentPointerDown(event: PointerEvent): void {
    if (this.viewerControl.brushPopoutAt() === null) {
      return;
    }
    if (!this.host.nativeElement.contains(event.target as Node)) {
      this.close();
    }
  }
}
