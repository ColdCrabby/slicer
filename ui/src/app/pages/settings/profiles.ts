import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import {
  ADHESION_TYPES,
  INFILL_PATTERNS,
  PRINT_QUALITIES,
  SEAM_POSITIONS,
  type AdhesionType,
  type InfillPattern,
  type PrintProfile,
  type PrintQuality,
  type SeamPosition,
} from '../../models/print-profile.model';
import { PROFILE_SOURCE_LABELS } from '../../models/profile-source';
import { CloudCatalog } from '../../services/catalog/cloud-catalog';
import { ActiveSelection } from '../../services/profiles/active-selection';
import {
  matchesAllLabels,
  toggledFilter,
  toggledLabelIds,
} from '../../services/profiles/label-filtering';
import { LabelsStore } from '../../services/profiles/labels-store';
import { PrintProfilesStore } from '../../services/profiles/print-profiles-store';
import { Icon } from '../../shared/icon/icon';
import { CatalogPicker, type CatalogEntryVm } from '../../components/profiles/catalog-picker';
import { ProfileWizard } from '../../components/profiles/profile-wizard';
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
import { Segmented } from '../../ui/segmented/segmented';
import { Select } from '../../ui/select/select';
import { Switch } from '../../ui/switch/switch';

@Component({
  selector: 'nexus-settings-profiles',
  imports: [
    SectionHeader,
    EmptyState,
    Button,
    IconButton,
    Icon,
    ProfileWizard,
    CatalogPicker,
    ModalShell,
    FieldRow,
    NumberInput,
    Select,
    Switch,
    Segmented,
    LabelChip,
    LabelFilterBar,
    LabelPicker,
  ],
  templateUrl: './profiles.html',
  styleUrl: './profiles.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ProfilesSettings {
  protected readonly store = inject(PrintProfilesStore);
  protected readonly active = inject(ActiveSelection);
  protected readonly labels = inject(LabelsStore);
  private readonly catalog = inject(CloudCatalog);

  protected readonly sourceLabels = PROFILE_SOURCE_LABELS;
  protected readonly qualityOptions = PRINT_QUALITIES.map((q) => ({ value: q, label: q }));
  protected readonly patternOptions = INFILL_PATTERNS;
  protected readonly seamOptions = SEAM_POSITIONS;
  protected readonly adhesionOptions = ADHESION_TYPES;

  protected readonly wizardOpen = signal(false);
  protected readonly catalogOpen = signal(false);
  protected readonly editingId = signal<string | null>(null);
  protected readonly confirmDeleteId = signal<string | null>(null);
  protected readonly labelFilter = signal<string[]>([]);

  /** Print profiles narrowed by the active label filter. */
  protected readonly visibleItems = computed(() =>
    this.store.items().filter((p) => matchesAllLabels(p, this.labelFilter())),
  );

  protected labelsOf(item: PrintProfile) {
    return this.labels.resolve(item.labelIds);
  }

  protected toggleFilter(id: string): void {
    this.labelFilter.update((f) => toggledFilter(f, id));
  }

  protected clearFilter(): void {
    this.labelFilter.set([]);
  }

  protected toggleLabel(id: string, labelId: string): void {
    const item = this.store.getById(id);
    if (item) {
      this.store.update(id, { labelIds: toggledLabelIds(item.labelIds, labelId) });
    }
  }

  protected readonly catalogStatus = this.catalog.status;
  protected readonly catalogEntries = computed<CatalogEntryVm[]>(() =>
    this.catalog.profiles().map((p) => ({
      id: p.id,
      name: p.name,
      vendor: p.quality,
      meta: `${p.layerHeight} mm · ${Math.round(p.infillDensity * 100)}% infill`,
      icon: 'menu-scale',
      imported: this.store.items().some((item) => item.basedOn === p.id),
    })),
  );

  protected infillPct(fraction: number): number {
    return Math.round(fraction * 100);
  }

  protected openWizard(): void {
    this.wizardOpen.set(true);
  }

  protected onWizardCompleted(profile: PrintProfile): void {
    this.store.add(profile);
    this.active.selectProfile(profile.id);
    this.editingId.set(profile.id);
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
    const entry = this.catalog.profiles().find((p) => p.id === id);
    if (!entry) {
      return;
    }
    const copy = this.store.importFromCatalog(entry);
    this.active.selectProfile(copy.id);
  }

  protected selectActive(id: string): void {
    this.active.selectProfile(id);
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

  protected update(id: string, patch: Partial<PrintProfile>): void {
    this.store.update(id, patch);
  }

  protected rename(id: string, event: Event): void {
    const name = (event.target as HTMLInputElement).value.trim();
    if (name) {
      this.store.update(id, { name });
    }
  }

  protected setInfillPercent(id: string, pct: number): void {
    this.store.update(id, { infillDensity: Math.max(0, Math.min(100, pct)) / 100 });
  }

  protected setQuality(id: string, value: string): void {
    this.store.update(id, { quality: value as PrintQuality });
  }

  protected setPattern(id: string, value: string): void {
    this.store.update(id, { infillPattern: value as InfillPattern });
  }

  protected setSeam(id: string, value: string): void {
    this.store.update(id, { seamPosition: value as SeamPosition });
  }

  protected setAdhesion(id: string, value: string): void {
    this.store.update(id, { adhesionType: value as AdhesionType });
  }
}
