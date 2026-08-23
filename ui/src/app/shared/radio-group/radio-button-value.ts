import { Directive, ElementRef, HostBinding, HostListener, inject, input } from '@angular/core';
import { RadioGroup } from './radio-group';

@Directive({
  selector: '[radioButtonValue]',
  exportAs: 'radioButtonValue',
})
export class RadioButtonValue {
  readonly radioButtonValue = input.required<unknown>();

  private readonly group = inject(RadioGroup);
  private readonly el = inject(ElementRef<HTMLElement>);

  @HostBinding('class.active')
  isActive = false;

  @HostBinding('attr.role')
  readonly role = 'radio';

  @HostBinding('attr.aria-checked')
  ariaChecked = 'false';

  @HostBinding('attr.tabindex')
  tabIndex = -1;

  @HostListener('click')
  onClick(): void {
    this.group.activate(this);
  }

  @HostListener('keydown', ['$event'])
  onKeyDown(event: KeyboardEvent): void {
    switch (event.key) {
      case 'ArrowRight':
      case 'ArrowDown':
        event.preventDefault();
        this.group.focusRelative(this, 1);
        return;
      case 'ArrowLeft':
      case 'ArrowUp':
        event.preventDefault();
        this.group.focusRelative(this, -1);
        return;
      case 'Home':
        event.preventDefault();
        this.group.focusFirst();
        return;
      case 'End':
        event.preventDefault();
        this.group.focusLast();
        return;
      case ' ':
      case 'Enter':
        event.preventDefault();
        this.group.activate(this);
        return;
      default:
        return;
    }
  }

  setStateFromGroup(active: boolean, focusable: boolean): void {
    this.isActive = active;
    this.ariaChecked = String(active);
    this.tabIndex = focusable ? 0 : -1;
  }

  focus(): void {
    this.el.nativeElement.focus();
  }
}
