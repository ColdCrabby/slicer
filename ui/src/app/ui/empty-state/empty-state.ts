import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { Icon } from '../../shared/icon/icon';

/**
 * Centered empty-state placeholder — an optional icon, a heading, an optional
 * description and a projected slot for a call-to-action.
 */
@Component({
  selector: 'nexus-empty-state',
  imports: [Icon],
  template: `
    @if (icon(); as name) {
      <div class="es-icon"><nexus-icon [name]="name" /></div>
    }
    <h3 class="es-title">{{ heading() }}</h3>
    @if (description(); as d) {
      <p class="es-desc">{{ d }}</p>
    }
    <div class="es-actions"><ng-content /></div>
  `,
  styleUrl: './empty-state.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class EmptyState {
  readonly icon = input<string>();
  readonly heading = input.required<string>();
  readonly description = input<string>();
}
