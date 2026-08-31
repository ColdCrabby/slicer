import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import {
  FILAMENT_MATERIAL_LABELS,
  FILAMENT_MATERIALS,
  makeFilament,
  MATERIAL_DENSITY,
  MATERIAL_PARAMS,
  type FilamentMaterial,
  type FilamentProfile,
} from '../../models/filament.model';
import { CloudCatalog, catalogSpecOf, toUserCopy } from '../../services/catalog/cloud-catalog';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { FilamentsStore } from '../../services/profiles/filaments-store';
import { NotificationService } from '../../services/notifications';
import { Icon, NumberInput, Select, ColorPicker, FieldRow, WizardShell } from '@coldcrabby/ui';
import { CatalogPicker, type CatalogEntryVm } from './catalog-picker';
import { paramNum } from '../../models/params-access';

const STEPS = ['Start', 'Basics', 'Temperatures', 'Cooling & flow'] as const;

/**
 * Guided flow for adding a filament. Picking a material in step 1 pre-fills the
 * temperature/cooling defaults for that material so a from-scratch spool still
 * lands on sane values.
 */
@Component({
  selector: 'nexus-filament-wizard',
  standalone: true,
  imports: [WizardShell, CatalogPicker, FieldRow, NumberInput, Select, ColorPicker, Icon],
  templateUrl: './filament-wizard.html',
  styleUrl: './filament-wizard.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class FilamentWizard {
  private readonly catalog = inject(CloudCatalog);
  private readonly store = inject(FilamentsStore);
  private readonly active = inject(ActiveSelection);
  private readonly notifications = inject(NotificationService);
  private readonly router = inject(Router);

  protected readonly steps = STEPS;
  protected readonly index = signal(0);
  protected readonly draft = signal<FilamentProfile>(makeFilament());

  protected readonly materialOptions = FILAMENT_MATERIALS.map((m) => ({
    value: m,
    label: FILAMENT_MATERIAL_LABELS[m],
  }));

  protected readonly catalogStatus = this.catalog.filamentsStatus;
  protected readonly catalogHasMore = this.catalog.filamentsHasMore;
  protected readonly catalogLoadingMore = this.catalog.filamentsLoadingMore;
  /** Id of the catalog entry currently being fetched for import, if any. */
  protected readonly importingId = signal<string | null>(null);
  protected readonly catalogEntries = computed<CatalogEntryVm[]>(() =>
    this.catalog.filaments().map((f) => ({
      id: f.id,
      name: f.name,
      vendor: f.vendor,
      meta:
        catalogSpecOf(f) ??
        `${f.material} · ${(f.params as Record<string, unknown>)?.['nozzle_temp']}°C`,
      color: f.color,
      imported: this.store.items().some((item) => item.based_on === f.id),
    })),
  );

  protected readonly canProceed = computed(() => {
    if (this.index() === 0) {
      return false;
    }
    return this.draft().name.trim().length > 0;
  });

  constructor() {
    void this.catalog.loadFilaments();
  }

  protected readonly pnum = paramNum;

  protected patch(patch: Partial<FilamentProfile>): void {
    this.draft.update((d) => ({ ...d, ...patch }));
  }

  /** Merge a partial `SlicingParams` into the draft's `params` bundle. */
  protected patchParams(patch: Record<string, unknown>): void {
    this.draft.update((d) => ({
      ...d,
      params: { ...((d.params as Record<string, unknown>) ?? {}), ...patch },
    }));
  }

  protected patchName(event: Event): void {
    this.patch({ name: (event.target as HTMLInputElement).value });
  }

  protected patchVendor(event: Event): void {
    this.patch({ vendor: (event.target as HTMLInputElement).value });
  }

  protected patchColor(color: string): void {
    this.patch({ color });
  }

  protected setMaterial(value: string): void {
    const material = value as FilamentMaterial;
    // Re-seed material-dependent slice params + density; keep identity/color.
    this.patch({
      material,
      density_g_cm3: MATERIAL_DENSITY[material],
      params: {
        ...((this.draft().params as Record<string, unknown>) ?? {}),
        ...MATERIAL_PARAMS[material],
      },
    });
  }

  protected startFromScratch(): void {
    this.draft.set(makeFilament());
    this.index.set(1);
  }

  /**
   * Fetch the full preset behind `id` (real slicing params, not just the
   * browsed summary) and seed the draft from it. The catalog picker shows a
   * busy state on this entry's pick button for the duration.
   */
  protected async startFromCatalog(id: string): Promise<void> {
    const base = this.catalog.filaments().find((f) => f.id === id);
    if (!base || this.importingId()) {
      return;
    }
    this.importingId.set(id);
    try {
      const full = await this.catalog.filamentDetail(base);
      this.draft.set(toUserCopy(full));
      this.index.set(1);
    } catch (error) {
      this.notifications.error(
        'Could not load preset',
        error instanceof Error ? error.message : 'The preset details could not be fetched.',
      );
    } finally {
      this.importingId.set(null);
    }
  }

  protected loadMoreCatalog(): void {
    void this.catalog.loadMoreFilaments();
  }

  protected onCatalogSearch(query: string): void {
    void this.catalog.searchFilaments(query);
  }

  protected retryCatalog(): void {
    void this.catalog.loadFilaments(true, this.catalog.filamentsQuery());
  }

  protected back(): void {
    this.index.update((i) => Math.max(0, i - 1));
  }

  protected next(): void {
    this.index.update((i) => Math.min(this.steps.length - 1, i + 1));
  }

  protected goto(index: number): void {
    this.index.set(index);
  }

  protected finish(): void {
    this.persist();
    void this.router.navigate(['/settings/filaments']);
  }

  /** Create the filament, then open its editor scrolled to the extra sections. */
  protected finishAndConfigure(): void {
    const filament = this.persist();
    void this.router.navigate(['/settings/filaments'], {
      queryParams: { configure: filament.id },
    });
  }

  /** Persist the draft and make it the active filament; returns the saved profile. */
  private persist(): FilamentProfile {
    const filament = this.draft();
    this.store.add(filament);
    this.active.selectFilament(filament.id);
    return filament;
  }

  protected cancel(): void {
    void this.router.navigate(['/settings/filaments']);
  }
}
