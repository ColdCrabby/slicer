import { CdkConnectedOverlay, CdkOverlayOrigin } from '@angular/cdk/overlay';
import type { ConnectedPosition } from '@angular/cdk/overlay';
import {
  ChangeDetectionStrategy,
  Component,
  computed,
  input,
  output,
  signal,
  viewChild,
} from '@angular/core';
import type { ElementRef } from '@angular/core';
import { Icon } from '../../shared/icon/icon';

export interface SelectOption {
  value: string;
  label: string;
  description?: string;
}

/**
 * Design-system dropdown. A custom listbox whose menu is styled to the Nexus
 * surface tokens and can show a secondary description per option. The menu is
 * rendered in a CDK connected overlay (body-level) so it is never clipped by
 * scrolling or `overflow: hidden` ancestors — solid surface, border + shadow,
 * no blur. Controlled: parent owns `value`.
 *
 * ```html
 * <nexus-select [options]="patterns" [value]="pattern()"
 *   (valueChange)="pattern.set($event)" />
 * ```
 */
@Component({
  selector: 'nexus-select',
  standalone: true,
  imports: [Icon, CdkOverlayOrigin, CdkConnectedOverlay],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './select.html',
  styleUrl: './select.scss',
  host: {
    '[class.is-open]': 'open()',
  },
})
export class Select {
  private readonly triggerEl = viewChild.required<ElementRef<HTMLElement>>('triggerEl');

  readonly options = input<readonly SelectOption[]>([]);
  readonly value = input<string | null>(null);
  readonly placeholder = input('Select…');
  readonly disabled = input(false);
  readonly valueChange = output<string>();

  protected readonly open = signal(false);
  protected readonly activeIndex = signal(-1);
  protected readonly menuWidth = signal(0);

  protected readonly positions: ConnectedPosition[] = [
    { originX: 'start', originY: 'bottom', overlayX: 'start', overlayY: 'top', offsetY: 4 },
    { originX: 'start', originY: 'top', overlayX: 'start', overlayY: 'bottom', offsetY: -4 },
  ];

  protected readonly selected = computed(
    () => this.options().find((o) => o.value === this.value()) ?? null,
  );

  protected toggle(): void {
    if (this.disabled()) return;
    this.open() ? this.close() : this.openMenu();
  }

  protected openMenu(): void {
    const current = this.options().findIndex((o) => o.value === this.value());
    this.activeIndex.set(current === -1 ? 0 : current);
    this.menuWidth.set(this.triggerEl().nativeElement.offsetWidth);
    this.open.set(true);
  }

  protected close(): void {
    this.open.set(false);
    this.activeIndex.set(-1);
  }

  protected pick(opt: SelectOption): void {
    if (opt.value !== this.value()) this.valueChange.emit(opt.value);
    this.close();
  }

  protected onTriggerKeydown(event: KeyboardEvent): void {
    if (this.disabled()) return;

    if (!this.open()) {
      if (event.key === 'Enter' || event.key === ' ' || event.key === 'ArrowDown') {
        event.preventDefault();
        this.openMenu();
      }
      return;
    }

    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        this.move(1);
        break;
      case 'ArrowUp':
        event.preventDefault();
        this.move(-1);
        break;
      case 'Enter':
      case ' ': {
        event.preventDefault();
        const opt = this.options()[this.activeIndex()];
        if (opt) this.pick(opt);
        break;
      }
      case 'Escape':
        event.preventDefault();
        this.close();
        break;
      case 'Tab':
        this.close();
        break;
    }
  }

  private move(dir: number): void {
    const count = this.options().length;
    if (count === 0) return;
    const next = (this.activeIndex() + dir + count) % count;
    this.activeIndex.set(next);
  }
}
