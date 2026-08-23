import { ChangeDetectionStrategy, Component, computed, inject, input, output } from '@angular/core';
import { LabelsStore } from '../../services/profiles/labels-store';
import { Icon } from '../../shared/icon/icon';

/**
 * A horizontal row of every label rendered as a toggle chip, used to filter a
 * profile list. Selected chips show the label's tint; unselected ones are a
 * quiet outline with a hue dot. The parent owns the selected-id set and the
 * actual filtering.
 *
 * Renders nothing when no labels exist, so pages that `@if` on its emptiness
 * can hide the whole bar.
 */
@Component({
  selector: 'nexus-label-filter-bar',
  standalone: true,
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './label-filter-bar.html',
  styleUrl: './label-filter-bar.scss',
})
export class LabelFilterBar {
  protected readonly store = inject(LabelsStore);

  readonly selectedIds = input<readonly string[]>([]);
  readonly toggle = output<string>();
  readonly clear = output<void>();

  protected readonly hasSelection = computed(() => this.selectedIds().length > 0);

  protected isSelected(id: string): boolean {
    return this.selectedIds().includes(id);
  }
}
