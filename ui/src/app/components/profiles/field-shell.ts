import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { MarkdownComponent } from 'ngx-markdown';

/**
 * The bare row primitive shared by every editor field: an optional title and a
 * projected control on one line, with the field's **description rendered inline
 * below** — the editor has the vertical room, so guidance stays visible instead
 * of hidden behind a cramped info tooltip.
 *
 * Purely presentational: it owns only the row rhythm (padding, the between-row
 * divider, title/control flex layout, the muted description treatment) and
 * knows nothing about schemas, profiles, or the store. Both the schema-driven
 * {@link ParamField} and bespoke editors (e.g. the labels page) wrap it so the
 * markup and styles for a settings row live in exactly one place.
 *
 * Give it a {@link title} and project the control via `<ng-content>`. Omit the
 * title and the control takes the full width, letting a section header (or a
 * `profile-editor__group-title`) label it instead. Set {@link stacked} for a
 * control that cannot share a line with its label — option cards, which are as
 * tall as their explanations.
 */
@Component({
  selector: 'nexus-field-shell',
  standalone: true,
  imports: [MarkdownComponent],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div
      class="field-shell"
      [class.field-shell--titled]="!!title()"
      [class.field-shell--stacked]="stacked()"
    >
      <div class="field-shell__row">
        @if (title(); as t) {
          <span class="field-shell__title">{{ t }}</span>
        }
        <span class="field-shell__control">
          <ng-content />
        </span>
      </div>
      @if (description(); as d) {
        <markdown class="field-shell__desc" [data]="d" />
      }
    </div>
  `,
  styles: [
    `
      :host {
        display: block;
      }
      :host + :host .field-shell {
        border-top: 1px solid var(--color-border-light);
      }
      .field-shell {
        padding: var(--spacing-md) 0;
      }
      .field-shell__row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-lg);
      }
      .field-shell__title {
        /* Keep enough room for a two-word label: a wide control used to squeeze
           the title until "Thumbnail Size" broke as "Thumbna / Size". */
        flex: 1;
        min-width: 7rem;
        font-size: var(--font-size-md);
        color: var(--color-text-primary);
      }
      .field-shell__control {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        flex: 1;
      }
      /* With a title the control hugs the right, giving up width before the
         label does; titleless rows span the row. */
      .field-shell--titled .field-shell__control {
        flex: 0 1 auto;
      }
      /* A stacked control drops under its label and takes the full width. */
      .field-shell--stacked .field-shell__row {
        flex-direction: column;
        align-items: stretch;
        gap: var(--spacing-sm);
      }
      .field-shell--stacked .field-shell__control {
        flex: 1;
      }
      /* The engine writes its descriptions as Markdown and they arrive that
         way, unaltered. ngx-markdown emits raw HTML, so those nodes never carry
         this component's scoping attribute and ::ng-deep is the only way to
         reach them — the same hook changelog-list.scss uses. */
      .field-shell__desc {
        display: block;
        margin: var(--spacing-xs) 0 0;
        max-width: 62ch;
        font-size: var(--font-size-xs);
        line-height: 1.5;
        color: var(--color-text-tertiary);
      }
      .field-shell__desc ::ng-deep > :first-child {
        margin-top: 0;
      }
      .field-shell__desc ::ng-deep > :last-child {
        margin-bottom: 0;
      }
      .field-shell__desc ::ng-deep p {
        margin: 0 0 var(--spacing-xs);
      }
      .field-shell__desc ::ng-deep ul,
      .field-shell__desc ::ng-deep ol {
        margin: 0 0 var(--spacing-xs);
        padding-left: var(--spacing-lg);
      }
      .field-shell__desc ::ng-deep li {
        margin-bottom: 2px;
      }
      .field-shell__desc ::ng-deep code {
        font-family: var(--font-family-mono);
        font-size: 0.95em;
      }
    `,
  ],
})
export class FieldShell {
  /** Left-hand label for the row. Omit to let a section header do the labelling. */
  readonly title = input('');
  /** Optional helper text rendered inline below the row, as Markdown. */
  readonly description = input('');
  /** Put the control under the label, full width, instead of beside it. */
  readonly stacked = input(false);
}
