import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  afterNextRender,
  input,
  output,
  viewChild,
} from '@angular/core';

/**
 * The name a wizard is about to save, edited in place at heading size.
 *
 * The last step of a wizard is the one place the name is the *subject* rather
 * than one field among several, so it is typed where it will be read — large,
 * unboxed, with a rule that lights up on focus — instead of in a form row that
 * makes naming look like the eighth thing to fill in.
 *
 * **The value is written to the DOM once.** A contenteditable that is re-bound
 * on every keystroke loses the caret to the end of the line, so this seeds the
 * element at first render, emits outward from then on, and only writes back
 * when the value changes from somewhere else while the field is not focused.
 */
@Component({
  selector: 'nexus-wizard-name',
  standalone: true,
  templateUrl: './wizard-name.html',
  styleUrl: './wizard-name.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WizardName {
  readonly value = input.required<string>();
  readonly placeholder = input('Name it');
  /** Describes the field for a screen reader, since there is no visible label. */
  readonly ariaLabel = input('Name');

  readonly valueChange = output<string>();

  private readonly field = viewChild.required<ElementRef<HTMLElement>>('field');

  constructor() {
    afterNextRender(() => {
      const element = this.field().nativeElement;
      element.textContent = this.value();
      // `plaintext-only` keeps pasted rich text out of a field that is really a
      // string; Firefox only learned it recently, so fall back where it is not
      // honoured rather than accepting markup.
      element.setAttribute(
        'contenteditable',
        element.contentEditable === 'plaintext-only' ? 'plaintext-only' : 'true',
      );
    });
  }

  protected onInput(): void {
    this.valueChange.emit(this.field().nativeElement.textContent?.trim() ?? '');
  }

  /** A name is one line: Enter commits it rather than opening a second. */
  protected onKeydown(event: KeyboardEvent): void {
    if (event.key === 'Enter') {
      event.preventDefault();
      this.field().nativeElement.blur();
    }
  }

  /**
   * Paste as text even where `plaintext-only` was not honoured, so a name
   * copied out of a web page does not arrive carrying its markup.
   */
  protected onPaste(event: ClipboardEvent): void {
    const text = event.clipboardData?.getData('text/plain');
    if (text == null) {
      return;
    }
    event.preventDefault();
    this.field().nativeElement.ownerDocument.execCommand(
      'insertText',
      false,
      text.replace(/\s+/g, ' '),
    );
  }
}
