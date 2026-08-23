import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { LABEL_HUES, type Label, type LabelTone } from '../../models/label.model';
import { Icon } from '../../shared/icon/icon';
import { LabelChip } from './label-chip';

/**
 * Compact label-colour chooser: a single row of muted base hues plus a
 * Dark / Light shade toggle, and a live preview chip. Deliberately tiny — a few
 * hues cover the spectrum and the tone toggle supplies the softer variant,
 * instead of a large free-form palette. Controlled by the parent.
 */
@Component({
  selector: 'nexus-color-swatch-picker',
  standalone: true,
  imports: [Icon, LabelChip],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './color-swatch-picker.html',
  styleUrl: './color-swatch-picker.scss',
})
export class ColorSwatchPicker {
  readonly hue = input.required<string>();
  readonly tone = input.required<LabelTone>();
  /** Name shown in the preview chip. */
  readonly previewName = input('Preview');

  readonly hueChange = output<string>();
  readonly toneChange = output<LabelTone>();

  protected readonly hues = LABEL_HUES;

  protected get previewLabel(): Label {
    return {
      id: 'preview',
      name: this.previewName() || 'Preview',
      color: this.hue(),
      tone: this.tone(),
    };
  }

  protected pickHue(value: string): void {
    this.hueChange.emit(value);
  }

  protected pickTone(tone: LabelTone): void {
    this.toneChange.emit(tone);
  }
}
