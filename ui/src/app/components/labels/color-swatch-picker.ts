import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { LABEL_HUES, LABEL_TONES, type Label, type LabelTone } from '../../models/label.model';
import { Icon } from '@coldcrabby/ui';
import { LabelChip } from './label-chip';

/**
 * Compact label-colour chooser rendered as two GitHub-style rows of the same
 * hues — a deeper `dark` row and a brighter `light` row. Picking a swatch sets
 * both hue and tone at once; a live preview chip shows the result. Controlled by
 * the parent.
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
  protected readonly tones = LABEL_TONES;

  protected get previewLabel(): Label {
    return {
      id: 'preview',
      name: this.previewName() || 'Preview',
      color: this.hue(),
      tone: this.tone(),
    };
  }

  protected pick(color: string, tone: LabelTone): void {
    if (color !== this.hue()) {
      this.hueChange.emit(color);
    }
    if (tone !== this.tone()) {
      this.toneChange.emit(tone);
    }
  }
}
