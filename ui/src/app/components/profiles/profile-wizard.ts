import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import {
  ADHESION_TYPES,
  INFILL_PATTERNS,
  makePrintProfile,
  PRINT_QUALITIES,
  SEAM_POSITIONS,
  type AdhesionType,
  type InfillPattern,
  type PrintProfile,
  type PrintQuality,
  type SeamPosition,
} from '../../models/print-profile.model';
import { CloudCatalog, toUserCopy } from '../../services/catalog/cloud-catalog';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { PrintProfilesStore } from '../../services/profiles/print-profiles-store';
import {
  Icon,
  NumberInput,
  Segmented,
  Select,
  Switch,
  FieldRow,
  WizardShell,
} from '@coldcrabby/ui';
import { CatalogPicker, type CatalogEntryVm } from './catalog-picker';
import { paramBool, paramNum, paramStr } from '../../models/params-access';

const STEPS = ['Start', 'Layers & walls', 'Infill', 'Speeds & supports'] as const;

/** Guided flow for adding a print (quality/process) profile. */
@Component({
  selector: 'nexus-profile-wizard',
  standalone: true,
  imports: [WizardShell, CatalogPicker, FieldRow, NumberInput, Select, Switch, Segmented, Icon],
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

  protected readonly qualityOptions = PRINT_QUALITIES.map((q) => ({ value: q, label: q }));
  protected readonly patternOptions = INFILL_PATTERNS;
  protected readonly seamOptions = SEAM_POSITIONS;
  protected readonly adhesionOptions = ADHESION_TYPES;

  protected readonly catalogStatus = this.catalog.status;
  protected readonly catalogEntries = computed<CatalogEntryVm[]>(() =>
    this.catalog.profiles().map((p) => {
      const params = (p.params as Record<string, unknown>) ?? {};
      const layer = Number(params['layer_height'] ?? 0);
      const infill = Number(params['infill_density'] ?? 0);
      return {
        id: p.id,
        name: p.name,
        vendor: p.quality ?? 'standard',
        meta: `${layer} mm · ${Math.round(infill * 100)}% infill`,
        icon: 'menu-scale',
        imported: this.store.items().some((item) => item.based_on === p.id),
      };
    }),
  );

  protected readonly infillPercent = computed(() =>
    Math.round(
      Number((this.draft().params as Record<string, unknown>)?.['infill_density'] ?? 0) * 100,
    ),
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

  protected readonly pnum = paramNum;
  protected readonly pstr = paramStr;
  protected readonly pbool = paramBool;

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

  protected setInfillPercent(pct: number): void {
    this.patchParams({ infill_density: Math.max(0, Math.min(100, pct)) / 100 });
  }

  protected setQuality(value: string): void {
    this.patch({ quality: value as PrintQuality });
  }

  protected setPattern(value: string): void {
    this.patchParams({ infill_pattern: value as InfillPattern });
  }

  protected setSeam(value: string): void {
    this.patchParams({ seam_position: value as SeamPosition });
  }

  protected setAdhesion(value: string): void {
    this.patchParams({ adhesion_type: value as AdhesionType });
  }

  protected startFromScratch(): void {
    this.draft.set(makePrintProfile());
    this.index.set(1);
  }

  protected startFromCatalog(id: string): void {
    const entry = this.catalog.profiles().find((p) => p.id === id);
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

  protected goto(index: number): void {
    this.index.set(index);
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
