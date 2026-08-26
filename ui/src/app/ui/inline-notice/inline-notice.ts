import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { Icon } from '../../shared/icon/icon';

/** Visual severity of an {@link InlineNotice}. */
export type InlineNoticeTone = 'info' | 'warning' | 'danger';

const TONE_ICONS: Record<InlineNoticeTone, string> = {
  info: 'info-circle',
  warning: 'warning-triangle',
  danger: 'warning-triangle',
};

/**
 * Compact inline callout — an icon, an optional bold title, and projected body
 * text — for surfacing a contextual note or caution next to the controls it
 * refers to. Sits on a solid tinted surface (no blur), tone-coloured by
 * severity. Purely presentational: it renders its inputs and nothing else.
 */
@Component({
  selector: 'nexus-inline-notice',
  imports: [Icon],
  template: `
    <nexus-icon class="inline-notice-icon" [name]="resolvedIcon()" aria-hidden="true" />
    <div class="inline-notice-body">
      @if (title(); as t) {
        <p class="inline-notice-title">{{ t }}</p>
      }
      <p class="inline-notice-text"><ng-content /></p>
    </div>
  `,
  styleUrl: './inline-notice.scss',
  host: {
    role: 'note',
    '[class]': '"inline-notice inline-notice--" + tone()',
  },
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class InlineNotice {
  /** Severity; drives the accent colour and default icon. */
  readonly tone = input<InlineNoticeTone>('info');
  /** Optional icon-name override; defaults to the tone's icon. */
  readonly icon = input<string>();
  /** Optional bold lead line above the projected body text. */
  readonly title = input<string>();

  protected readonly resolvedIcon = computed(() => this.icon() ?? TONE_ICONS[this.tone()]);
}
