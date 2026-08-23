import { Directive, HostBinding, contentChildren, effect, model } from '@angular/core';
import { RadioButtonValue } from './radio-button-value';

@Directive({
  selector: '[radioGroup]',
  exportAs: 'radioGroup',
})
export class RadioGroup {
  @HostBinding('attr.role')
  readonly role = 'radiogroup';

  readonly value = model<unknown>(null, { alias: 'radioGroup' });

  private readonly buttons = contentChildren(RadioButtonValue, { descendants: true });

  constructor() {
    effect(() => {
      this.value();
      this.buttons();
      this.syncButtons();
    });
  }

  select(value: unknown): void {
    this.value.set(value);
  }

  activate(button: RadioButtonValue): void {
    const value = this.getButtonValue(button);
    if (value === undefined) {
      return;
    }

    this.select(value);
  }

  focusRelative(button: RadioButtonValue, delta: number): void {
    const buttons = this.buttons();
    if (buttons.length === 0) {
      return;
    }

    const index = buttons.indexOf(button);
    if (index < 0) {
      return;
    }

    const nextIndex = (index + delta + buttons.length) % buttons.length;
    this.activateAndFocusIndex(nextIndex);
  }

  focusFirst(): void {
    this.activateAndFocusIndex(0);
  }

  focusLast(): void {
    const buttons = this.buttons();
    if (buttons.length === 0) {
      return;
    }

    this.activateAndFocusIndex(buttons.length - 1);
  }

  private activateAndFocusIndex(index: number): void {
    const buttons = this.buttons();
    if (buttons.length === 0 || index < 0 || index >= buttons.length) {
      return;
    }

    const button = buttons[index];
    this.activate(button);
    button.focus();
  }

  private syncButtons(): void {
    const buttons = this.buttons();
    if (buttons.length === 0) {
      return;
    }

    const current = this.value();
    const selectedIndex = buttons.findIndex((button) => this.getButtonValue(button) === current);
    const focusIndex = selectedIndex >= 0 ? selectedIndex : 0;

    for (let index = 0; index < buttons.length; index += 1) {
      const button = buttons[index];
      button.setStateFromGroup(index === selectedIndex, index === focusIndex);
    }
  }

  private getButtonValue(button: RadioButtonValue): unknown | undefined {
    try {
      return button.radioButtonValue();
    } catch {
      // Required input may not be bound yet; the effect reruns after binding.
      return undefined;
    }
  }
}
