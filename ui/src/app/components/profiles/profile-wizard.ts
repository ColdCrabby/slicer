import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import {
  makePrintProfile,
  PRINT_QUALITIES,
  PRINT_QUALITY_LABELS,
  type PrintProfile,
  type PrintQuality,
} from '../../models/print-profile.model';
import { CloudCatalog, catalogSpecOf, toUserCopy } from '../../services/catalog/cloud-catalog';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { PrintProfilesStore } from '../../services/profiles/print-profiles-store';
import { Icon, NumberInput, Segmented, FieldRow } from '@coldcrabby/ui';
import { CatalogPicker, type CatalogEntryVm } from './catalog-picker';
import { WizardChrome, type WizardAction } from './wizard-chrome';
import { WizardRoute } from './wizard-route';
import { WizardName } from './wizard-name';
import { paramNum } from '../../models/params-access';

/**
 * Two steps, because there are only two things this flow knows that the profile
 * editor does not: where the profile should start from, and what to call it.
 *
 * Everything else is rendered by the editor straight from the `SlicingParams`
 * schema — grouped, tiered and searchable, so a new parameter appears without a
 * line of TypeScript. The wizard's four pages of fields were a second,
 * hand-maintained copy of that list: it drifted from the schema, ignored the
 * tier model, and asked someone creating their first profile about the infill
 * angle and the seam position with the same weight as the layer height.
 */
const STEPS = ['Where to start', 'Name it'] as const;

/** Guided flow for adding a print (quality/process) profile. */
@Component({
  selector: 'nexus-profile-wizard',
  standalone: true,
  imports: [
    WizardChrome,
    WizardRoute,
    WizardName,
    CatalogPicker,
    FieldRow,
    NumberInput,
    Segmented,
    Icon,
  ],
  templateUrl: './profile-wizard.html',
  styleUrl: './profile-wizard.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ProfileWizard {
  private readonly catalog = inject(CloudCatalog);
  private readonly store = inject(PrintProfilesStore);
  private readonly active = inject(ActiveSelection);
  private readonly router = inject(Router);

  protected readonly steps = STEPS;
  protected readonly index = signal(0);
  protected readonly draft = signal<PrintProfile>(makePrintProfile());

  protected readonly qualityOptions = PRINT_QUALITIES.map((quality) => ({
    value: quality,
    label: PRINT_QUALITY_LABELS[quality],
  }));

  protected readonly catalogStatus = this.catalog.profilesStatus;
  protected readonly catalogHasMore = this.catalog.profilesHasMore;
  protected readonly catalogLoadingMore = this.catalog.profilesLoadingMore;
  /** Id of the catalog entry currently being fetched for import, if any. */
  protected readonly importingId = signal<string | null>(null);

  /**
   * Why the last catalog import failed. Rendered by the picker, beside the
   * button that was pressed — an error about a control the user is looking at
   * does not belong in a floating message somewhere else.
   */
  protected readonly importError = signal<string | null>(null);
  protected readonly catalogEntries = computed<CatalogEntryVm[]>(() =>
    this.catalog.profiles().map((p) => {
      const params = (p.params as Record<string, unknown>) ?? {};
      const layer = Number(params['layer_height'] ?? 0);
      const infill = Number(params['infill_density'] ?? 0);
      return {
        id: p.id,
        name: p.name,
        vendor: p.quality ?? 'standard',
        meta: catalogSpecOf(p) ?? `${layer} mm · ${Math.round(infill * 100)}% infill`,
        icon: 'menu-scale',
        imported: this.store.items().some((item) => item.based_on === p.id),
      };
    }),
  );

  /** Whether the catalog route on the first screen is unfolded. */
  protected readonly presetsOpen = signal(false);

  protected togglePresets(): void {
    this.presetsOpen.update((open) => !open);
  }

  protected readonly named = computed(() => this.draft().name.trim().length > 0);

  /**
   * The footer's actions. The first step advances by choosing a starting point,
   * so it offers none — a Next there could only be a disabled button with
   * nothing that would enable it.
   */
  protected readonly actions = computed<WizardAction[]>(() =>
    this.index() === 0
      ? []
      : [
          { id: 'configure', label: 'Add & configure', disabled: !this.named() },
          { id: 'finish', label: 'Add profile', disabled: !this.named() },
        ],
  );

  protected onAction(id: string): void {
    if (id === 'finish') {
      this.finish();
    } else if (id === 'configure') {
      this.finishAndConfigure();
    }
  }

  constructor() {
    void this.catalog.loadProfiles();
  }

  protected readonly pnum = paramNum;

  protected patch(patch: Partial<PrintProfile>): void {
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

  protected setQuality(value: string): void {
    this.patch({ quality: value as PrintQuality });
  }

  protected startFromScratch(): void {
    this.draft.set(makePrintProfile());
    this.index.set(1);
  }

  /**
   * Fetch the full preset behind `id` (real slicing params, not just the
   * browsed summary) and seed the draft from it. The catalog picker shows a
   * busy state on this entry's pick button for the duration.
   */
  protected async startFromCatalog(id: string): Promise<void> {
    const base = this.catalog.profiles().find((p) => p.id === id);
    if (!base || this.importingId()) {
      return;
    }
    this.importingId.set(id);
    this.importError.set(null);
    try {
      const full = await this.catalog.profileDetail(base);
      this.draft.set(toUserCopy(full));
      this.index.set(1);
    } catch (error) {
      this.importError.set(
        error instanceof Error ? error.message : 'The preset details could not be fetched.',
      );
    } finally {
      this.importingId.set(null);
    }
  }

  protected loadMoreCatalog(): void {
    void this.catalog.loadMoreProfiles();
  }

  protected onCatalogSearch(query: string): void {
    void this.catalog.searchProfiles(query);
  }

  protected retryCatalog(): void {
    void this.catalog.loadProfiles(true, this.catalog.profilesQuery());
  }

  protected back(): void {
    this.index.update((i) => Math.max(0, i - 1));
  }

  protected finish(): void {
    this.persist();
    void this.router.navigate(['/settings/profiles']);
  }

  /** Create the profile, then open its editor scrolled to the extra sections. */
  protected finishAndConfigure(): void {
    const profile = this.persist();
    void this.router.navigate(['/settings/profiles'], {
      queryParams: { configure: profile.id },
    });
  }

  /** Persist the draft and make it the active profile; returns the saved profile. */
  private persist(): PrintProfile {
    const profile = this.draft();
    this.store.add(profile);
    this.active.selectProfile(profile.id);
    return profile;
  }

  protected cancel(): void {
    void this.router.navigate(['/settings/profiles']);
  }
}
