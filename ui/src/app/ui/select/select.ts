import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  computed,
  inject,
  input,
  output,
  signal,
  viewChild,
} from '@angular/core';
import type { ElementRef, TemplateRef } from '@angular/core';
import { FloatingService } from '../../shared/floating';
import type { FloatingRef } from '../../shared/floating';
import { Icon } from '../../shared/icon/icon';

export interface SelectOption {
  value: string;
  label: string;
  description?: string;
  /** Optional label colours shown as small dots (e.g. profile labels). */
  swatches?: string[];
}

/**
 * Design-system dropdown. A custom listbox whose menu is styled to the Nexus
 * surface tokens and can show a secondary description per option. The menu is
 * rendered through the shared FloatingService (body-level) so it is never
 * clipped by scrolling or `overflow: hidden` ancestors, flips/shifts to stay
 * on-screen, and fits its height to the available space. Controlled: parent
 * owns `value`.
 *
 * ```html
 * <nexus-select [options]="patterns" [value]="pattern()"
 *   (valueChange)="pattern.set($event)" />
 * ```
 */
@Component({
  selector: 'nexus-select',
  standalone: true,
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './select.html',
  styleUrl: './select.scss',
  host: {
    '[class.is-open]': 'open()',
  },
})
export class Select {
  private readonly triggerEl = viewChild.required<ElementRef<HTMLElement>>('triggerEl');
  private readonly menuTpl = viewChild.required<TemplateRef<unknown>>('menuTpl');

  private readonly floating = inject(FloatingService);
  private floatingRef: FloatingRef | null = null;

  readonly options = input<readonly SelectOption[]>([]);
  readonly value = input<string | null>(null);
  readonly placeholder = input('Select…');
  readonly disabled = input(false);
  /** Optional leading icon (iconoir name) shown at the start of the trigger. */
  readonly startIcon = input<string | null>(null);
  readonly valueChange = output<string>();

  protected readonly open = signal(false);
  protected readonly activeIndex = signal(-1);

  protected readonly selected = computed(
    () => this.options().find((o) => o.value === this.value()) ?? null,
  );

  constructor() {
    inject(DestroyRef).onDestroy(() => this.closeMenu());
  }

  protected toggle(): void {
    if (this.disabled()) return;
    this.open() ? this.close() : this.openMenu();
  }

  protected openMenu(): void {
    const current = this.options().findIndex((o) => o.value === this.value());
    this.activeIndex.set(current === -1 ? 0 : current);
    this.open.set(true);

    const trigger = this.triggerEl().nativeElement;
    this.floatingRef = this.floating.openTemplate(
      this.menuTpl(),
      {},
      {
        reference: trigger,
        interactive: true,
        panelClass: 'nexus-floating--fit',
        originElement: trigger,
        options: {
          placement: 'bottom-start',
          offset: 4,
          padding: 8,
          size: true,
          matchReferenceWidth: true,
        },
        onOutsidePointer: () => this.close(),
        onEscape: () => this.close(),
      },
    );
  }

  protected close(): void {
    this.open.set(false);
    this.activeIndex.set(-1);
    this.closeMenu();
  }

  private closeMenu(): void {
    this.floatingRef?.close();
    this.floatingRef = null;
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
