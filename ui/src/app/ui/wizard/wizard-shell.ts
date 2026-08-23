import { ChangeDetectionStrategy, Component, computed, input, output } from '@angular/core';
import { Icon } from '../../shared/icon/icon';

/**
 * Presentational chrome for a multi-step "add" flow: a modal card with a header
 * (title + numbered step indicator + close), a scrolling body (`<ng-content>`),
 * and a footer with Back / Cancel / Next-or-Finish.
 *
 * Owns no state — the parent wizard drives {@link index}, decides when the
 * current step is valid ({@link canProceed}), and renders the right fields for
 * the current step. This keeps the chrome reusable across printer / filament /
 * profile wizards.
 */
@Component({
  selector: 'nexus-wizard-shell',
  standalone: true,
  imports: [Icon],
  templateUrl: './wizard-shell.html',
  styleUrl: './wizard-shell.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WizardShell {
  readonly title = input.required<string>();
  readonly steps = input.required<readonly string[]>();
  readonly index = input.required<number>();
  readonly canProceed = input(true);
  readonly finishLabel = input('Create');

  readonly back = output<void>();
  readonly next = output<void>();
  readonly cancel = output<void>();
  readonly finish = output<void>();

  protected readonly isFirst = computed(() => this.index() === 0);
  protected readonly isLast = computed(() => this.index() === this.steps().length - 1);
  protected readonly stepList = computed(() =>
    this.steps().map((label, i) => ({
      label,
      i,
      done: i < this.index(),
      active: i === this.index(),
    })),
  );

  protected onPrimary(): void {
    if (this.isLast()) {
      this.finish.emit();
    } else {
      this.next.emit();
    }
  }
}
