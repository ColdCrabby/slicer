import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { isValidHexColor, LABEL_PALETTE, randomLabelColor } from '../../models/label.model';
import { Icon } from '../../shared/icon/icon';

/**
 * GitHub-style colour chooser: a preview + shuffle button, a grid of curated
 * palette swatches, and a free-form hex input. Controlled — the parent owns the
 * `value` and updates it from `valueChange`.
 */
@Component({
  selector: 'nexus-color-swatch-picker',
  standalone: true,
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './color-swatch-picker.html',
  styleUrl: './color-swatch-picker.scss',
})
export class ColorSwatchPicker {
  readonly value = input.required<string>();
  readonly valueChange = output<string>();

  protected readonly palette = LABEL_PALETTE;

  protected pick(color: string): void {
    this.valueChange.emit(color);
  }

  protected shuffle(): void {
    this.valueChange.emit(randomLabelColor());
  }

  protected onHexInput(event: Event): void {
    let raw = (event.target as HTMLInputElement).value.trim();
    if (raw && !raw.startsWith('#')) {
      raw = `#${raw}`;
    }
    if (isValidHexColor(raw)) {
      this.valueChange.emit(raw.toLowerCase());
    }
  }

  protected onNativeInput(event: Event): void {
    this.valueChange.emit((event.target as HTMLInputElement).value.toLowerCase());
  }
}
