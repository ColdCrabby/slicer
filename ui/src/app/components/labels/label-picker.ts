import { CdkConnectedOverlay, CdkOverlayOrigin } from '@angular/cdk/overlay';
import type { ConnectedPosition } from '@angular/cdk/overlay';
import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  input,
  output,
  signal,
} from '@angular/core';
import { labelDotColor, makeLabel, type Label } from '../../models/label.model';
import { LabelsStore } from '../../services/profiles/labels-store';
import { Icon } from '../../shared/icon/icon';
import { LabelChip } from './label-chip';

/**
 * Attach / detach labels on a single profile. Renders the currently-assigned
 * labels inline plus an "Add" trigger that opens a popover listing every label
 * with a checkmark, a search box, and an inline "create new label" affordance.
 *
 * The parent owns the profile's `labelIds`; this component emits {@link toggle}
 * (a label id) whenever one is added or removed. Creating a new label writes to
 * {@link LabelsStore} and immediately emits `toggle` to assign it.
 */
@Component({
  selector: 'nexus-label-picker',
  standalone: true,
  imports: [Icon, LabelChip, CdkOverlayOrigin, CdkConnectedOverlay],
  templateUrl: './label-picker.html',
  styleUrl: './label-picker.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LabelPicker {
  protected readonly store = inject(LabelsStore);

  readonly assignedIds = input<readonly string[]>([]);
  readonly toggle = output<string>();

  protected readonly open = signal(false);
  protected readonly query = signal('');

  protected readonly positions: ConnectedPosition[] = [
    { originX: 'start', originY: 'bottom', overlayX: 'start', overlayY: 'top', offsetY: 4 },
    { originX: 'start', originY: 'top', overlayX: 'start', overlayY: 'bottom', offsetY: -4 },
  ];

  protected readonly assignedLabels = computed(() => this.store.resolve(this.assignedIds()));

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

  protected close(): void {
    this.open.set(false);
    this.query.set('');
  }
}
