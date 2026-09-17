import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  computed,
  effect,
  inject,
  input,
  output,
  viewChild,
} from '@angular/core';
import { Button, Icon } from '@coldcrabby/ui';

/** One action in the chrome's footer, beside Back and Cancel. */
export interface WizardAction {
  /** Button label. */
  readonly label: string;
  /** Emitted through {@link WizardChrome.act} when pressed. */
  readonly id: string;
  /** Trailing iconoir glyph. */
  readonly icon?: string;
  /** Greyed out, for an action the current step has not earned yet. */
  readonly disabled?: boolean;
}

/** How far a step change moved, so the body can slide the matching way. */
type Direction = 1 | -1;

/** Travel of the body's slide, in pixels. */
const SLIDE_PX = 16;

/**
 * Presentational chrome for the profile wizards: a title, a progress bar, a
 * scrolling body, and one row of actions.
 *
 * **A progress bar, not a stepper.** These flows do not have a plan the user
 * can see up front — the printer wizard rebuilds its steps around whatever a
 * detection could not settle, so a numbered strip promises a shape it does not
 * know yet, and at seven steps it outgrew its own container. A bar plus
 * "Step 2 of 7 · Extra fan" survives any count, says the same thing, and leaves
 * the eye on the question instead of the map.
 *
 * Owns no state beyond the slide direction: the parent drives {@link index},
 * names the steps, and decides which actions the current step offers.
 */
@Component({
  selector: 'nexus-wizard-chrome',
  standalone: true,
  imports: [Button, Icon],
  templateUrl: './wizard-chrome.html',
  styleUrl: './wizard-chrome.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WizardChrome {
  readonly title = input.required<string>();
  /** Step names, longest-lived first. Only the current one is ever shown. */
  readonly steps = input.required<readonly string[]>();
  readonly index = input.required<number>();
  /** Back is offered whenever there is somewhere to go back to. */
  readonly canGoBack = input(true);
  /**
   * The footer's actions, primary last — the row renders them left to right and
   * styles the final one as the primary. An empty list leaves only Cancel.
   */
  readonly actions = input<readonly WizardAction[]>([]);
  /**
   * Whether to show the bar and the step count.
   *
   * False on a screen that chooses a route rather than advancing along one:
   * until the user picks, there is no journey to be one-of-four through, and
   * the printer wizard's real answer is four, eight or anything between
   * depending on what a detection could not settle. A count printed before it
   * is known is a guess dressed as chrome.
   */
  readonly showProgress = input(true);

  readonly cancel = output<void>();
  readonly back = output<void>();
  /** The {@link WizardAction.id} of whichever action was pressed. */
  readonly act = output<string>();

  private readonly body = viewChild<ElementRef<HTMLElement>>('body');

  protected readonly stepLabel = computed(() => this.steps()[this.index()] ?? '');
  protected readonly total = computed(() => Math.max(1, this.steps().length));
  /**
   * Fill fraction, as a percentage.
   *
   * The first step is already a step taken, so a one-of-four start shows a
   * quarter rather than an empty bar — an untouched track reads as "nothing has
   * happened yet" on a screen the user is plainly looking at.
   */
  protected readonly percent = computed(() =>
    Math.round(((this.index() + 1) / this.total()) * 100),
  );

  constructor() {
    // `null` until the first run: a required input cannot be read before the
    // effect fires, and the opening step should not slide in as if it replaced
    // something.
    let previous: number | null = null;
    effect(() => {
      const current = this.index();
      const from = previous;
      previous = current;
      if (from !== null) {
        this.slide(current >= from ? 1 : -1);
      }
    });
  }

  protected onAction(action: WizardAction): void {
    if (!action.disabled) {
      this.act.emit(action.id);
    }
  }

  /**
   * Slide the new step in from the side it came from.
   *
   * Driven from the Web Animations API rather than a CSS class, because a step
   * can repeat the same index (a question answered, then re-entered) and a
   * class toggle needs a forced reflow to replay. Honours the user's
   * reduced-motion setting, and does nothing at all before the first render.
   */
  private slide(direction: Direction): void {
    const element = this.body()?.nativeElement;
    if (!element || matchMedia('(prefers-reduced-motion: reduce)').matches) {
      return;
    }
    element.animate(
      [
        { opacity: 0, transform: `translateX(${direction * SLIDE_PX}px)` },
        { opacity: 1, transform: 'translateX(0)' },
      ],
      { duration: 180, easing: 'cubic-bezier(0.2, 0, 0, 1)' },
    );
  }
}
