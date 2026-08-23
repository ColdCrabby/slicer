import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import {
  FILAMENT_MATERIAL_LABELS,
  FILAMENT_MATERIALS,
  MATERIAL_PRESETS,
  type FilamentMaterial,
  type FilamentProfile,
} from '../../models/filament.model';
import { PROFILE_SOURCE_LABELS } from '../../models/profile-source';
import { CloudCatalog } from '../../services/catalog/cloud-catalog';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { matchesAllLabels, toggledLabelIds } from '../../services/profiles/label-filtering';
import { LabelFilterStore } from '../../services/profiles/label-filter-store';
import { LabelsStore } from '../../services/profiles/labels-store';
import { FilamentsStore } from '../../services/profiles/filaments-store';
import { Icon } from '../../shared/icon/icon';
import { CatalogPicker, type CatalogEntryVm } from '../../components/profiles/catalog-picker';
import { FilamentWizard } from '../../components/profiles/filament-wizard';
import { LabelChip } from '../../components/labels/label-chip';
import { LabelFilterBar } from '../../components/labels/label-filter-bar';
import { LabelPicker } from '../../components/labels/label-picker';
import { Button } from '../../ui/button/button';
import { EmptyState } from '../../ui/empty-state/empty-state';
import { FieldRow } from '../../ui/field-row/field-row';
import { IconButton } from '../../ui/icon-button/icon-button';
import { ModalShell } from '../../ui/modal-shell/modal-shell';
import { NumberInput } from '../../ui/number-input/number-input';
import { SectionHeader } from '../../ui/section-header/section-header';
import { Select } from '../../ui/select/select';
import { Switch } from '../../ui/switch/switch';

@Component({
  selector: 'nexus-settings-filaments',
  imports: [
    SectionHeader,
    EmptyState,
    Button,
    IconButton,
    Icon,
    FilamentWizard,
    CatalogPicker,
    ModalShell,
    FieldRow,
    NumberInput,
    Select,
    Switch,
    LabelChip,
    LabelFilterBar,
    LabelPicker,
  ],
  templateUrl: './filaments.html',
  styleUrl: './filaments.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class FilamentsSettings {
  protected readonly store = inject(FilamentsStore);
  protected readonly active = inject(ActiveSelection);
  protected readonly labels = inject(LabelsStore);
  private readonly filterStore = inject(LabelFilterStore);
  private readonly catalog = inject(CloudCatalog);

  protected readonly sourceLabels = PROFILE_SOURCE_LABELS;
  protected readonly materialOptions = FILAMENT_MATERIALS.map((m) => ({
    value: m,
    label: FILAMENT_MATERIAL_LABELS[m],
  }));

  protected readonly wizardOpen = signal(false);
  protected readonly catalogOpen = signal(false);
  protected readonly editingId = signal<string | null>(null);
  protected readonly confirmDeleteId = signal<string | null>(null);
  protected readonly labelFilter = this.filterStore.selectedIds;

  /** Filaments narrowed by the active label filter. */
  protected readonly visibleItems = computed(() =>
    this.store.items().filter((f) => matchesAllLabels(f, this.labelFilter())),
  );

  protected labelsOf(item: FilamentProfile) {
    return this.labels.resolve(item.labelIds);
  }

  protected toggleFilter(id: string): void {
    this.filterStore.toggle(id);
  }

  protected clearFilter(): void {
    this.filterStore.clear();
  }

  protected toggleLabel(id: string, labelId: string): void {
    const item = this.store.getById(id);
    if (item) {
      this.store.update(id, { labelIds: toggledLabelIds(item.labelIds, labelId) });
    }
  }

  protected readonly catalogStatus = this.catalog.status;
  protected readonly catalogEntries = computed<CatalogEntryVm[]>(() =>
    this.catalog.filaments().map((f) => ({
      id: f.id,
      name: f.name,
      vendor: f.vendor,
      meta: `${f.material} · ${f.nozzleTemp}°C`,
      color: f.color,
      imported: this.store.items().some((item) => item.basedOn === f.id),
    })),
  );

  protected openWizard(): void {
    this.wizardOpen.set(true);
  }

  protected onWizardCompleted(filament: FilamentProfile): void {
    this.store.add(filament);
    this.active.selectFilament(filament.id);
    this.editingId.set(filament.id);
    this.wizardOpen.set(false);
  }

  protected openCatalog(): void {
    void this.catalog.load();
    this.catalogOpen.set(true);
  }

  protected retryCatalog(): void {
    void this.catalog.load(true);
  }

  protected importFromCatalog(id: string): void {
    const entry = this.catalog.filaments().find((f) => f.id === id);
    if (!entry) {
      return;
    }
    const copy = this.store.importFromCatalog(entry);
    this.active.selectFilament(copy.id);
  }

  protected selectActive(id: string): void {
    this.active.selectFilament(id);
  }

  protected toggleEditor(id: string): void {
    this.editingId.update((current) => (current === id ? null : id));
  }

  protected duplicate(id: string): void {
    const copy = this.store.duplicate(id);
    if (copy) {
      this.editingId.set(copy.id);
    }
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

  protected update(id: string, patch: Partial<FilamentProfile>): void {
    this.store.update(id, patch);
  }

  protected rename(id: string, event: Event): void {
    const name = (event.target as HTMLInputElement).value.trim();
    if (name) {
      this.store.update(id, { name });
    }
  }

  protected setColor(id: string, event: Event): void {
    this.store.update(id, { color: (event.target as HTMLInputElement).value });
  }

  protected setMaterial(id: string, value: string): void {
    const material = value as FilamentMaterial;
    this.store.update(id, { material, ...MATERIAL_PRESETS[material] });
  }
}
