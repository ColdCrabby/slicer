import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { makeLabel, type Label, type LabelTone } from '../../models/label.model';
import { ColorSwatchPicker } from '../../components/labels/color-swatch-picker';
import { FieldShell } from '../../components/profiles/field-shell';
import { LabelChip } from '../../components/labels/label-chip';
import { ContextMenuService } from '../../services/context-menu/context-menu.service';
import type { ContextMenuItem } from '../../services/context-menu/context-menu.model';
import { FilamentsStore } from '../../services/profiles/filaments-store';
import { LabelsStore } from '../../services/profiles/labels-store';
import { PrintProfilesStore } from '../../services/profiles/print-profiles-store';
import { PrintersStore } from '../../services/profiles/printers-store';
import { Icon } from '../../shared/icon/icon';
import { Button } from '../../ui/button/button';
import { EmptyState } from '../../ui/empty-state/empty-state';
import { IconButton } from '../../ui/icon-button/icon-button';
import { SectionHeader } from '../../ui/section-header/section-header';

@Component({
  selector: 'nexus-settings-labels',
  imports: [
    SectionHeader,
    EmptyState,
    Button,
    IconButton,
    Icon,
    LabelChip,
    ColorSwatchPicker,
    FieldShell,
  ],
  templateUrl: './labels.html',
  styleUrl: './labels.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LabelsSettings {
  protected readonly store = inject(LabelsStore);
  private readonly printers = inject(PrintersStore);
  private readonly filaments = inject(FilamentsStore);
  private readonly profiles = inject(PrintProfilesStore);
  private readonly contextMenu = inject(ContextMenuService);

  protected readonly editingId = signal<string | null>(null);
  protected readonly confirmDeleteId = signal<string | null>(null);

  /** How many profiles across all areas currently use each label. */
  protected readonly usage = computed(() => {
    const counts = new Map<string, number>();
    const bump = (ids: string[] | undefined) => {
      for (const id of ids ?? []) {
        counts.set(id, (counts.get(id) ?? 0) + 1);
      }
    };
    for (const p of this.printers.items()) bump(p.label_ids);
    for (const f of this.filaments.items()) bump(f.label_ids);
    for (const p of this.profiles.items()) bump(p.label_ids);
    return counts;
  });

  protected usageFor(id: string): number {
    return this.usage().get(id) ?? 0;
  }

  protected add(): void {
    const label = this.store.add(makeLabel({ name: '' }));
    this.editingId.set(label.id);
  }

  protected toggleEditor(id: string): void {
    this.editingId.update((current) => (current === id ? null : id));
  }

  /** Right-click a label row: edit, copy its colour, or delete. */
  protected onContextMenu(event: MouseEvent, label: Label): void {
    const items: ContextMenuItem[] = [
      { label: 'Edit', icon: 'edit-pencil', action: () => this.editingId.set(label.id) },
      {
        label: 'Copy colour',
        icon: 'copy',
        action: () => void navigator.clipboard?.writeText(label.color),
      },
      { separator: true, label: '' },
      {
        label: 'Delete',
        icon: 'trash',
        danger: true,
        action: () => {
          this.store.remove(label.id);
          if (this.editingId() === label.id) {
            this.editingId.set(null);
          }
        },
      },
    ];
    void this.contextMenu.open(event, items);
  }

  protected rename(id: string, event: Event): void {
    this.store.update(id, { name: (event.target as HTMLInputElement).value });
  }

  protected setHue(id: string, color: string): void {
    this.store.update(id, { color });
  }

  protected setTone(id: string, tone: LabelTone): void {
    this.store.update(id, { tone });
  }

  protected requestDelete(id: string): void {
    if (this.confirmDeleteId() === id) {
      this.store.remove(id);
      this.confirmDeleteId.set(null);
      if (this.editingId() === id) {
        this.editingId.set(null);
      }
    } else {
      this.confirmDeleteId.set(id);
      setTimeout(() => {
        if (this.confirmDeleteId() === id) {
          this.confirmDeleteId.set(null);
        }
      }, 3000);
    }
  }

  protected chipFor(label: Label): Label {
    return label.name.trim() ? label : { ...label, name: 'Unnamed' };
  }
}
