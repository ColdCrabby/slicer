import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { RouterLink } from '@angular/router';
import { Arrange, MAX_ARRANGE_SPACING_MM, MIN_ARRANGE_SPACING_MM } from '../../services/arrange';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { ViewerControl } from '../../services/viewer-control';
import { Icon } from '../../shared/icon/icon';
import { TooltipDirective } from '../../shared/tooltip/tooltip.directive';
import { NumberInput } from '../../ui/number-input/number-input';
import { Switch } from '../../ui/switch/switch';

/**
 * Contextual placement settings, hanging off the placement tool.
 *
 * The object-mode buttons each reveal a card of sub-settings for the tool they
 * turn on ({@link TransformPanel}); the placement button is in that same group
 * and behaves the same way — this is its card. Both are rendered by the
 * toolbar, anchored under the buttons that open them, so the card is visibly
 * attached to the control that produced it rather than parked in a corner.
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
  protected readonly preferredOrientationDeg = this.arrange.preferredOrientationDeg;
  protected readonly objectCount = this.arrange.objectCount;

  /** Hidden in G-code preview for the same reason the toolbar's plate tools are. */
  protected readonly visible = computed(
    () => this.arrange.optionsOpen() && this.viewerControl.viewMode() === 'model',
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

  protected close(): void {
    this.arrange.closeOptions();
  }
}
