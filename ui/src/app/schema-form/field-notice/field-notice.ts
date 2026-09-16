import { ChangeDetectionStrategy, Component, inject, input } from '@angular/core';
import { RouterLink } from '@angular/router';
import { InlineNotice } from '@coldcrabby/ui';
import type { FieldNotice, FieldNoticeLink } from '../field-exceptions/field-exceptions';
import { ActivePresets } from '../../services/profiles/active-presets';

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
          <a
            class="field-notice-link"
            [routerLink]="link.routerLink"
            [queryParams]="queryParamsFor(link)"
            >{{ link.text }}</a
          >
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
  private readonly presets = inject(ActivePresets);

  readonly notice = input<FieldNotice | null>(null);

  /**
   * Where the link lands, beyond the page.
   *
   * A notice exists because something elsewhere needs attention, so stopping at
   * the page and leaving the user to find one control among sixty is most of
   * the job undone. `configure` opens the profile the setting actually lives on
   * — the one active on this plate, not whichever the editor last showed — and
   * `focus` scrolls to the control and flashes it.
   *
   * Resolving the profile here rather than in the exception table keeps that
   * table declarative: an entry names a contract and a setting, and never has
   * to know what is selected.
   */
  protected queryParamsFor(link: FieldNoticeLink): Record<string, string> | null {
    if (!link.setting) {
      return null;
    }
    const params: Record<string, string> = { focus: link.setting };
    const id = link.contract ? this.presets.selectedId(link.contract) : null;
    if (id) {
      params['configure'] = id;
    }
    return params;
  }
}
