import { ChangeDetectionStrategy, Component, input } from '@angular/core';

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
 * `profile-editor__group-title`) label it instead.
 */
@Component({
  selector: 'nexus-field-shell',
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="field-shell" [class.field-shell--titled]="!!title()">
      <div class="field-shell__row">
        @if (title(); as t) {
          <span class="field-shell__title">{{ t }}</span>
        }
        <span class="field-shell__control">
          <ng-content />
        </span>
      </div>
      @if (description(); as d) {
        <p class="field-shell__desc">{{ d }}</p>
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
        min-width: 0;
        font-size: var(--font-size-md);
        color: var(--color-text-primary);
      }
      .field-shell__control {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        flex: 1;
      }
      /* With a title the control hugs the right; titleless rows span the width. */
      .field-shell--titled .field-shell__control {
        flex: none;
      }
      .field-shell__desc {
        margin: var(--spacing-xs) 0 0;
        max-width: 62ch;
        font-size: var(--font-size-xs);
        line-height: 1.5;
        color: var(--color-text-tertiary);
        white-space: pre-line;
      }
    `,
  ],
})
export class FieldShell {
  /** Left-hand label for the row. Omit to let a section header do the labelling. */
  readonly title = input('');
  /** Optional helper text rendered inline below the row. */
  readonly description = input('');
}
