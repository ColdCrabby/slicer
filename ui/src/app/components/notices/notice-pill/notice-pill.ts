import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  computed,
  inject,
  input,
  output,
  signal,
  viewChild,
} from '@angular/core';
import { Icon, IconButton, TooltipDirective } from '../../../ui/shell-primitives';
import type { Notice, NoticeTone } from '../../../services/notifications';

const TONE_ICONS: Record<NoticeTone, string> = {
  info: 'info-circle',
  success: 'check-circle',
  warning: 'warning-triangle',
  danger: 'xmark-circle',
};

/**
 * One notice, drawn.
 *
 * The app has exactly one shape for an announcement — an icon, the message and
 * at most one thing to do about it — so that a message reads the same whether
 * it is floating over the plate or docked at the bottom of the window, and so
 * the same event can never be told twice in two visual languages.
 *
 * A running job fills the pill from behind rather than growing a separate bar:
 * the text stays put, and finishing is a colour change in the thing already
 * being watched instead of a new element somewhere else.
 *
 * Purely presentational — it renders a {@link Notice} and emits intent.
 */
@Component({
  selector: 'nexus-notice-pill',
  standalone: true,
  imports: [Icon, IconButton, TooltipDirective],
  templateUrl: './notice-pill.html',
  styleUrl: './notice-pill.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    '[class]': '"notice notice--" + notice().tone',
    '[class.is-wrapped]': 'wrapped()',
    // `alert` is implicitly assertive, so every routine success used to cut a
    // screen reader off mid-word. Only a genuine failure earns the interruption.
    '[attr.role]': 'notice().tone === "danger" ? "alert" : "status"',
    '(mouseenter)': 'hold.emit()',
    '(mouseleave)': 'release.emit()',
    '(focusin)': 'hold.emit()',
    '(focusout)': 'release.emit()',
  },
})
export class NoticePill {
  readonly notice = input.required<Notice>();

  /** The user asked for the notice's one action. */
  readonly act = output<void>();
  /** The user put the notice away — which, for a prompt, is the other answer. */
  readonly dismiss = output<void>();
  /** Pointer or focus entered; hold the countdown. */
  readonly hold = output<void>();
  /** …and left; resume it. */
  readonly release = output<void>();

  protected readonly icon = computed(() => {
    const notice = this.notice();
    return notice.icon ?? TONE_ICONS[notice.tone];
  });

  /**
   * True once the message has taken more than one line, which is what switches
   * the capsule to a rounded card.
   *
   * Observed rather than predicted: whether a line breaks depends on the text,
   * the font the host OS resolved, and how much width the dock has left — none
   * of which this component can compute, and all of which change under the
   * user (a resized window, a longer second message in the same notice).
   */
  protected readonly wrapped = signal(false);

  /**
   * True when even two lines were not enough and the text is ellipsised.
   *
   * The clamp has to exist — a stack trace would otherwise grow a card over
   * the plate — but a message the user cannot finish reading is not a message.
   * Hover restores the rest of it, on the same gesture that already holds the
   * countdown.
   */
  protected readonly clamped = signal(false);

  /** The whole message, for the tooltip a clamped notice needs. */
  protected readonly fullText = computed(() => {
    const notice = this.notice();
    return notice.message ? `${notice.title} · ${notice.message}` : notice.title;
  });

  private readonly label = viewChild.required<ElementRef<HTMLElement>>('label');

  private readonly destroyRef = inject(DestroyRef);

  constructor() {
    // `afterNextRender`: the view child does not exist during construction, and
    // measuring text before the browser has laid it out answers nothing.
    afterNextRender(() => {
      const element = this.label().nativeElement;
      const observer = new ResizeObserver((entries) => {
        const height = entries[0]?.contentRect.height ?? 0;
        const lineHeight = parseFloat(getComputedStyle(element).lineHeight) || 0;
        // Half a line of tolerance, so a sub-pixel line box never reads as a
        // second line. The element's own height is the measurement to take:
        // `-webkit-line-clamp` caps `scrollHeight`, so that would not see it.
        this.wrapped.set(lineHeight > 0 && height > lineHeight * 1.5);
        this.clamped.set(element.scrollHeight > element.clientHeight + 1);
      });
      observer.observe(element);
      this.destroyRef.onDestroy(() => observer.disconnect());
    });
  }
}
