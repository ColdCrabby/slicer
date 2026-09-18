import { ChangeDetectionStrategy, Component, computed, HostListener, inject } from '@angular/core';
import { RouterLink } from '@angular/router';
import { Arrange, MAX_ARRANGE_SPACING_MM, MIN_ARRANGE_SPACING_MM } from '../../services/arrange';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { ViewerControl } from '../../services/viewer-control';
import { Icon, TooltipDirective, NumberInput, Switch } from '@coldcrabby/ui';

/**
 * Contextual placement settings, hanging off the placement tool.
 *
 * The object tools each reveal a card of sub-settings for the tool they turn
 * on ({@link TransformPanel}); placing is one of those tools and this is its
 * card. All of them are docked together down the left edge of the scene by
 * the shell, and each names its own tool in its header — which is what ties
 * it back to the button, now that it no longer hangs underneath one.
 *
 * The machine's preferred print angle is **shown but not edited here** — it
 * belongs to the printer profile, so Settings owns it and this card links
 * there. A second editor would let one printer's angle be changed from a
 * surface that looks plate-scoped.
 */
@Component({
  selector: 'nexus-placement-panel',
  standalone: true,
  imports: [Icon, TooltipDirective, NumberInput, Switch, RouterLink],
  templateUrl: './placement-panel.html',
  styleUrl: './placement-panel.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class PlacementPanel {
  private readonly arrange = inject(Arrange);
  private readonly activeSelection = inject(ActiveSelection);
  private readonly viewerControl = inject(ViewerControl);

  protected readonly minSpacing = MIN_ARRANGE_SPACING_MM;
  protected readonly maxSpacing = MAX_ARRANGE_SPACING_MM;

  protected readonly spacingMm = this.arrange.spacingMm;
  protected readonly autoOrient = this.arrange.autoOrient;
  protected readonly turnToFit = this.arrange.turnToFit;
  protected readonly preferredOrientationDeg = this.arrange.preferredOrientationDeg;
  protected readonly objectCount = this.arrange.objectCount;

  /**
   * Showing exactly while placing is the active tool. Hidden in G-code preview
   * for the same reason the toolbar's plate tools are.
   */
  protected readonly visible = computed(
    () => this.viewerControl.objectMode() === 'place' && this.viewerControl.viewMode() === 'model',
  );

  /** Printer the preferred angle is stored on. */
  protected readonly printerName = computed(
    () => this.activeSelection.printer()?.name ?? 'this printer',
  );

  protected readonly actionLabel = computed(() =>
    this.objectCount() > 1 ? `Place ${this.objectCount()} objects` : 'Place on the bed',
  );

  /** The angle as shown in the read-out — `Off` reads better than `0°`. */
  protected readonly preferredLabel = computed(() => {
    const deg = this.preferredOrientationDeg();
    return deg === 0 ? 'Off' : `${deg}°`;
  });

  /**
   * Why the angle is or is not doing anything right now. It only applies when
   * auto-orient runs, so saying so beats leaving a live-looking value that has
   * no effect.
   */
  protected readonly preferredHint = computed(() => {
    if (!this.autoOrient()) {
      return 'Needs auto-orient.';
    }
    return this.preferredOrientationDeg() === 0
      ? `No extra turn on ${this.printerName()}.`
      : `Extra turn after orienting, from ${this.printerName()}.`;
  });

  protected run(): void {
    this.arrange.run();
  }

  protected setSpacing(value: number): void {
    this.arrange.setSpacingMm(value);
  }

  protected setAutoOrient(value: boolean): void {
    this.arrange.setAutoOrient(value);
  }

  protected setTurnToFit(value: boolean): void {
    this.arrange.setTurnToFit(value);
  }

  /**
   * Leave the placing tool for the harmless one.
   *
   * There is no "no tool" to fall back to, so closing this card means picking
   * another — and select-and-move is the one that changes nothing on its own.
   * Escape reaches it as well, like every other thing floating over the plate
   * (the brush popout, the settings peek); guarded, or Escape while rotating
   * would quietly switch tools on the way to clearing the selection.
   */
  @HostListener('document:keydown.escape')
  protected close(): void {
    if (!this.visible()) {
      return;
    }
    this.viewerControl.objectMode.set('translate');
  }
}
