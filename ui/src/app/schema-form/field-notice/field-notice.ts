import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { RouterLink } from '@angular/router';
import { InlineNotice } from '@coldcrabby/ui';
import type { FieldNotice } from '../field-exceptions/field-exceptions';

/**
 * Renders one {@link FieldNotice} — the tone-coloured caution a field's
 * exception produces, plus its optional link to wherever the notice can be
 * acted on.
 *
 * Exists because a field is hosted by **two** different per-item hosts: the
 * schema form's `FieldHost` (slice sidebar) and `ParamField` (the profile
 * editors). Both must render the same caution — one that appeared in only one of
 * them would be worse than none, since its absence elsewhere would read as
 * "this setting is fine". Keeping the markup here lets the registry decide
 * *what* to say while this component decides *how it looks*, once.
 *
 * Purely presentational: it renders its input and nothing else.
 */
@Component({
  selector: 'se-field-notice',
  standalone: true,
  imports: [InlineNotice, RouterLink],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (notice(); as n) {
      <nexus-inline-notice [tone]="n.tone ?? 'info'" [icon]="n.icon" [title]="n.title">
        {{ n.text }}
        @if (n.link; as link) {
          <a class="field-notice-link" [routerLink]="link.routerLink">{{ link.text }}</a>
        }
      </nexus-inline-notice>
    }
  `,
  styles: `
    :host {
      display: block;
    }

    .field-notice-link {
      display: inline-block;
      margin-top: 2px;
      color: var(--accent);
      text-decoration: none;

      &:hover {
        text-decoration: underline;
      }

      &:focus-visible {
        outline: 2px solid var(--color-focus-ring);
        outline-offset: 2px;
        border-radius: var(--radius-sm);
      }
    }
  `,
})
export class FieldNoticeView {
  /** The notice to render; `null` renders nothing. */
  readonly notice = input<FieldNotice | null>(null);
}
