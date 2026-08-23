import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  computed,
  inject,
  input,
  output,
} from '@angular/core';

export interface RadioOption {
  value: string;
  label: string;
  description?: string;
}

/**
 * Design-system single-choice group rendered as selectable option cards, each
 * with a custom radio dot and an optional description line. Follows the ARIA
 * radiogroup pattern (roving tabindex, arrow-key navigation). Controlled: the
 * parent owns `value` and updates it from `valueChange`.
 *
 * ```html
 * <nexus-radio-group [options]="modes" [value]="mode()"
 *   (valueChange)="mode.set($event)" />
 * ```
 */
@Component({
  selector: 'nexus-radio-group',
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './radio-group.html',
  styleUrl: './radio-group.scss',
  host: {
    role: 'radiogroup',
    '[attr.aria-label]': 'label() || null',
  },
})
export class RadioGroup {
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  readonly options = input<readonly RadioOption[]>([]);
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

  protected pick(opt: RadioOption): void {
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
      case 'ArrowDown':
      case 'ArrowRight':
        next = (i + 1) % count;
        break;
      case 'ArrowUp':
      case 'ArrowLeft':
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
    this.host.nativeElement.querySelectorAll<HTMLElement>('.option')[next]?.focus();
  }
}
