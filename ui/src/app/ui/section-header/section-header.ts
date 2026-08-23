import { ChangeDetectionStrategy, Component, input } from '@angular/core';

/**
 * A titled section heading with an optional description and a projected slot
 * for trailing actions (buttons, links).
 *
 * ```html
 * <nexus-section-header heading="Printers" description="Manage your machines">
 *   <a nexusButton variant="ghost" routerLink="/settings/printers">Manage</a>
 * </nexus-section-header>
 * ```
 */
@Component({
  selector: 'nexus-section-header',
  template: `
    <div class="sh-text">
      <h2 class="sh-title">{{ heading() }}</h2>
      @if (description(); as d) {
        <p class="sh-desc">{{ d }}</p>
      }
    </div>
    <div class="sh-actions"><ng-content /></div>
  `,
  styleUrl: './section-header.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SectionHeader {
  readonly heading = input.required<string>();
  readonly description = input<string>();
}
