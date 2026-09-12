import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  afterNextRender,
  computed,
  inject,
  input,
  output,
  signal,
  viewChild,
} from '@angular/core';
import { Icon } from '@coldcrabby/ui';
import { labelDotColor, makeLabel, type Label } from '../../models/label.model';
import { LabelsStore } from '../../services/profiles/labels-store';

/**
 * The label list itself — search, a checkable row per label, and an inline
 * "create this one" for a name that does not exist yet.
 *
 * Split out of {@link LabelPicker} so the same list can be opened from two
 * places: the picker's own trigger in a profile's detail pane, and the context
 * menu on a profile card. Putting the labels *into* the context menu instead
 * was the obvious thing and the wrong one — a menu cannot search, and a shelf
 * with twenty labels turns into a menu with twenty items.
 *
 * Holds no state about *where* it is shown; the caller owns that.
 */
@Component({
  selector: 'nexus-label-picker-panel',
  standalone: true,
  imports: [Icon],
  templateUrl: './label-picker-panel.html',
  styleUrl: './label-picker-panel.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LabelPickerPanel {
  protected readonly store = inject(LabelsStore);

  readonly assignedIds = input<readonly string[]>([]);
  /** Emits a label id whenever one is added or removed. */
  readonly toggle = output<string>();

  protected readonly query = signal('');
  private readonly search = viewChild<ElementRef<HTMLInputElement>>('search');

  constructor() {
    // The list exists to be filtered, and it is opened by a deliberate gesture
    // — so the caret starts where the user is going to type anyway.
    afterNextRender(() => this.search()?.nativeElement.focus({ preventScroll: true }));
  }

  protected readonly filtered = computed(() => {
    const q = this.query().trim().toLowerCase();
    const all = this.store.items();
    return q ? all.filter((l) => l.name.toLowerCase().includes(q)) : all;
  });

  /** Exact-match check so the "Create" row only shows for genuinely new names. */
  protected readonly canCreate = computed(() => {
    const q = this.query().trim();
    return (
      q.length > 0 && !this.store.items().some((l) => l.name.toLowerCase() === q.toLowerCase())
    );
  });

  protected isAssigned(id: string): boolean {
    return this.assignedIds().includes(id);
  }

  protected dotColor(label: Label): string {
    return labelDotColor(label);
  }

  protected onSearch(event: Event): void {
    this.query.set((event.target as HTMLInputElement).value);
  }

  protected toggleLabel(id: string): void {
    this.toggle.emit(id);
  }

  protected createAndAssign(): void {
    const name = this.query().trim();
    if (!name) {
      return;
    }
    const label = this.store.add(makeLabel({ name }));
    this.toggle.emit(label.id);
    this.query.set('');
  }
}
