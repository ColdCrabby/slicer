import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  computed,
  inject,
  input,
  output,
} from '@angular/core';
import { StackWhenCramped } from '../../shared/radio-group/stack-when-cramped';
import { Icon } from '../../shared/icon/icon';
import { TooltipDirective } from '../../shared/tooltip/tooltip.directive';

export interface SegmentOption {
  value: string;
  label: string;
  description?: string;
  /** Optional leading icon (iconoir name) shown before the label. */
  icon?: string;
}

/**
 * Design-system segmented control — a recessed track of equal-width options
 * with a single raised "pill" marking the selection. Native-OS feel, coherent
 * with the other field controls (same height/border tokens). Follows the ARIA
 * radiogroup pattern (roving tabindex, arrow-key navigation). Controlled: the
 * parent owns `value` and updates it from `valueChange`.
 *
 * ```html
 * <nexus-segmented [options]="patterns" [value]="pattern()"
 *   (valueChange)="pattern.set($event)" />
 * ```
 */
@Component({
  selector: 'nexus-segmented',
  standalone: true,
  imports: [TooltipDirective, Icon],
  hostDirectives: [StackWhenCramped],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './segmented.html',
  styleUrl: './segmented.scss',
  host: {
    role: 'radiogroup',
    '[attr.aria-label]': 'label() || null',
  },
})
export class Segmented {
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  readonly options = input<readonly SegmentOption[]>([]);
  readonly value = input<string | null>(null);
  readonly disabled = input(false);
  readonly label = input('');
  readonly valueChange = output<string>();

  protected readonly selectedIndex = computed(() =>
    this.options().findIndex((o) => o.value === this.value()),
  );

  protected tabIndexFor(i: number): number {
    const sel = this.selectedIndex();
    return (sel === -1 ? 0 : sel) === i ? 0 : -1;
  }

  protected pick(opt: SegmentOption): void {
    if (this.disabled()) return;
    if (opt.value !== this.value()) this.valueChange.emit(opt.value);
  }

  protected onKeydown(event: KeyboardEvent, i: number): void {
    if (this.disabled()) return;
    const opts = this.options();
    const count = opts.length;
    if (count === 0) return;

    let next: number;
    switch (event.key) {
      case 'ArrowRight':
      case 'ArrowDown':
        next = (i + 1) % count;
        break;
      case 'ArrowLeft':
      case 'ArrowUp':
        next = (i - 1 + count) % count;
        break;
      case 'Home':
        next = 0;
        break;
      case 'End':
        next = count - 1;
        break;
      case ' ':
      case 'Enter':
        event.preventDefault();
        this.pick(opts[i]);
        return;
      default:
        return;
    }

    event.preventDefault();
    this.pick(opts[next]);
    this.host.nativeElement.querySelectorAll<HTMLElement>('.segment')[next]?.focus();
  }
}
