import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  output,
  signal,
} from '@angular/core';
import {
  FILAMENT_MATERIAL_LABELS,
  FILAMENT_MATERIALS,
  makeFilament,
  MATERIAL_PRESETS,
  type FilamentMaterial,
  type FilamentProfile,
} from '../../models/filament.model';
import { CloudCatalog, toUserCopy } from '../../services/catalog/cloud-catalog';
import { FilamentsStore } from '../../services/profiles/filaments-store';
import { Icon } from '../../shared/icon/icon';
import { NumberInput } from '../../ui/number-input/number-input';
import { Select } from '../../ui/select/select';
import { Switch } from '../../ui/switch/switch';
import { FieldRow } from '../../ui/field-row/field-row';
import { WizardShell } from '../../ui/wizard/wizard-shell';
import { CatalogPicker, type CatalogEntryVm } from './catalog-picker';

const STEPS = ['Start', 'Basics', 'Temperatures', 'Cooling & flow'] as const;

/**
 * Guided flow for adding a filament. Picking a material in step 1 pre-fills the
 * temperature/cooling defaults for that material so a from-scratch spool still
 * lands on sane values.
 */
@Component({
  selector: 'nexus-filament-wizard',
  standalone: true,
  imports: [WizardShell, CatalogPicker, FieldRow, NumberInput, Select, Switch, Icon],
  templateUrl: './filament-wizard.html',
  styleUrl: './filament-wizard.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class FilamentWizard {
  private readonly catalog = inject(CloudCatalog);
  private readonly store = inject(FilamentsStore);

  readonly completed = output<FilamentProfile>();
  readonly cancelled = output<void>();

  protected readonly steps = STEPS;
  protected readonly index = signal(0);
  protected readonly draft = signal<FilamentProfile>(makeFilament());

  protected readonly materialOptions = FILAMENT_MATERIALS.map((m) => ({
    value: m,
    label: FILAMENT_MATERIAL_LABELS[m],
  }));

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

  protected readonly canProceed = computed(() => {
    if (this.index() === 0) {
      return false;
    }
    return this.draft().name.trim().length > 0;
  });

  constructor() {
    void this.catalog.load();
  }

  protected patch(patch: Partial<FilamentProfile>): void {
    this.draft.update((d) => ({ ...d, ...patch }));
  }

  protected patchName(event: Event): void {
    this.patch({ name: (event.target as HTMLInputElement).value });
  }

  protected patchVendor(event: Event): void {
    this.patch({ vendor: (event.target as HTMLInputElement).value });
  }

  protected patchColor(event: Event): void {
    this.patch({ color: (event.target as HTMLInputElement).value });
  }

  protected setMaterial(value: string): void {
    const material = value as FilamentMaterial;
    // Re-seed material-dependent defaults but keep identity/color/diameter.
    this.patch({ material, ...MATERIAL_PRESETS[material] });
  }

  protected startFromScratch(): void {
    this.draft.set(makeFilament());
    this.index.set(1);
  }

  protected startFromCatalog(id: string): void {
    const entry = this.catalog.filaments().find((f) => f.id === id);
    if (entry) {
      this.draft.set(toUserCopy(entry));
      this.index.set(1);
    }
  }

  protected retryCatalog(): void {
    void this.catalog.load(true);
  }

  protected back(): void {
    this.index.update((i) => Math.max(0, i - 1));
  }

  protected next(): void {
    this.index.update((i) => Math.min(this.steps.length - 1, i + 1));
  }

  protected finish(): void {
    this.completed.emit(this.draft());
  }
}
