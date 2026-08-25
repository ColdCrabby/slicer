import { ChangeDetectionStrategy, Component, afterNextRender, computed, inject, signal } from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
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
import { ContextMenuService } from '../../services/context-menu/context-menu.service';
import type { ContextMenuItem } from '../../services/context-menu/context-menu.model';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { matchesAllLabels, toggledLabelIds } from '../../services/profiles/label-filtering';
import { paramBool, paramNum, paramStr } from '../../models/params-access';
import { LabelFilterStore } from '../../services/profiles/label-filter-store';
import { LabelsStore } from '../../services/profiles/labels-store';
import { PrintProfilesStore } from '../../services/profiles/print-profiles-store';
import { Icon } from '../../shared/icon/icon';
import { Badge } from '../../shared/badge/badge';
import { CatalogPicker, type CatalogEntryVm } from '../../components/profiles/catalog-picker';
import { LabelFilterBar } from '../../components/labels/label-filter-bar';
import { LabelPicker } from '../../components/labels/label-picker';
import { focusConfigureTarget } from './configure-scroll';
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
    Badge,
    RouterLink,
    CatalogPicker,
    ModalShell,
    FieldRow,
    NumberInput,
    Select,
    Switch,
    Segmented,
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
  private readonly filterStore = inject(LabelFilterStore);
  private readonly catalog = inject(CloudCatalog);
  private readonly contextMenu = inject(ContextMenuService);
  private readonly route = inject(ActivatedRoute);

  protected readonly sourceLabels = PROFILE_SOURCE_LABELS;
  protected readonly qualityOptions = PRINT_QUALITIES.map((q) => ({ value: q, label: q }));
  protected readonly patternOptions = INFILL_PATTERNS;
  protected readonly seamOptions = SEAM_POSITIONS;
  protected readonly adhesionOptions = ADHESION_TYPES;
  protected readonly groupByOptions = [
    { value: 'category', label: 'Quality' },
    { value: 'label', label: 'Labels' },
    { value: 'none', label: 'None' },
  ];

  protected readonly catalogOpen = signal(false);
  /** Which profile's editor is open in the detail pane. */
  protected readonly selectedId = signal<string | null>(this.active.profile()?.id ?? null);
  protected readonly search = signal('');
  protected readonly groupBy = signal<'category' | 'label' | 'none'>('category');
  protected readonly labelFilter = this.filterStore.selectedIds;

  /** Typed-name delete challenge state (high-impact delete — design language). */
  protected readonly deleteArmed = signal(false);
  protected readonly deleteText = signal('');

  /** Print profiles narrowed by the active label filter and the search query. */
  protected readonly filtered = computed(() => {
    const q = this.search().trim().toLowerCase();
    return this.store.items().filter(
      (p) =>
        matchesAllLabels(p, this.labelFilter()) &&
        (!q || `${p.name} ${p.quality ?? ''}`.toLowerCase().includes(q)),
    );
  });

  /** The filtered profiles bucketed into titled groups per the group-by mode. */
  protected readonly groups = computed<{ key: string; title: string; items: PrintProfile[] }[]>(
    () => {
      const items = this.filtered();
      switch (this.groupBy()) {
        case 'none':
          return [{ key: 'all', title: '', items: sortByName(items) }];
        case 'label': {
          const groups: { key: string; title: string; items: PrintProfile[] }[] = [];
          for (const label of this.labels.items()) {
            const members = items.filter((p) => p.label_ids?.includes(label.id));
            if (members.length) {
              groups.push({ key: label.id, title: label.name, items: sortByName(members) });
            }
          }
          const unlabeled = items.filter((p) => !p.label_ids?.length);
          if (unlabeled.length) {
            groups.push({ key: '__none', title: 'Unlabeled', items: sortByName(unlabeled) });
          }
          return groups;
        }
        default: {
          // Group by quality tier, ordered by the canonical quality list.
          return PRINT_QUALITIES.map((quality) => ({
            key: quality,
            title: quality,
            items: sortByName(items.filter((p) => (p.quality ?? 'standard') === quality)),
          })).filter((g) => g.items.length > 0);
        }
      }
    },
  );

  protected readonly selected = computed(() => {
    const id = this.selectedId();
    return id ? (this.store.getById(id) ?? null) : null;
  });

  /** Whether the typed name matches the selected profile's name exactly. */
  protected readonly deleteReady = computed(() => {
    const p = this.selected();
    return !!p && this.deleteText().trim() === p.name.trim();
  });

  constructor() {
    // Arriving from the wizard's "Add & configure": open the new profile and
    // scroll to the full editor so the user can keep tuning it.
    const configureId = this.route.snapshot.queryParamMap.get('configure');
    if (configureId && this.store.getById(configureId)) {
      this.select(configureId);
      afterNextRender(() => focusConfigureTarget('configure-target'));
    }
  }

  protected setSearch(event: Event): void {
    this.search.set((event.target as HTMLInputElement).value);
  }

  protected clearSearch(): void {
    this.search.set('');
  }

  protected setGroupBy(value: string): void {
    this.groupBy.set(value as 'category' | 'label' | 'none');
  }

  protected isDefault(id: string): boolean {
    return this.active.profile()?.id === id;
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
      this.store.update(id, { label_ids: toggledLabelIds(item.label_ids, labelId) });
    }
  }

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

  protected infillPct(fraction: number): number {
    return Math.round(fraction * 100);
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
    this.select(copy.id);
  }

  /** Open a profile in the detail pane. */
  protected select(id: string): void {
    this.selectedId.set(id);
    this.disarmDelete();
  }

  /** Make the selected profile the default used for slicing. */
  protected setDefault(id: string): void {
    this.active.selectProfile(id);
  }

  protected duplicate(id: string): void {
    const copy = this.store.duplicate(id);
    if (copy) {
      this.select(copy.id);
    }
  }

  /** Right-click a profile card: quick actions mirroring the detail-pane buttons. */
  protected onContextMenu(event: MouseEvent, profile: PrintProfile): void {
    const items: ContextMenuItem[] = [
      {
        label: 'Set as default',
        icon: 'star',
        disabled: this.isDefault(profile.id),
        action: () => this.setDefault(profile.id),
      },
      { label: 'Duplicate', icon: 'copy', action: () => this.duplicate(profile.id) },
    ];
    if (profile.source !== 'builtin') {
      items.push({ separator: true, label: '' });
      items.push({
        label: 'Delete\u2026',
        icon: 'trash',
        danger: true,
        action: () => {
          this.select(profile.id);
          this.armDelete();
        },
      });
    }
    void this.contextMenu.open(event, items);
  }

  protected armDelete(): void {
    this.deleteArmed.set(true);
    this.deleteText.set('');
  }

  protected disarmDelete(): void {
    this.deleteArmed.set(false);
    this.deleteText.set('');
  }

  protected setDeleteText(event: Event): void {
    this.deleteText.set((event.target as HTMLInputElement).value);
  }

  /** Delete the selected profile once its name has been typed to confirm. */
  protected confirmDelete(): void {
    const profile = this.selected();
    if (!profile || !this.deleteReady()) {
      return;
    }
    this.store.remove(profile.id);
    this.disarmDelete();
    this.selectedId.set(this.store.items()[0]?.id ?? null);
  }

  protected readonly pnum = paramNum;
  protected readonly pstr = paramStr;
  protected readonly pbool = paramBool;

  protected update(id: string, patch: Partial<PrintProfile>): void {
    this.store.update(id, patch);
  }

  /** Merge a partial `SlicingParams` into a stored profile's `params` bundle. */
  protected updateParams(id: string, patch: Record<string, unknown>): void {
    const item = this.store.getById(id);
    if (item) {
      this.store.update(id, {
        params: { ...((item.params as Record<string, unknown>) ?? {}), ...patch },
      });
    }
  }

  protected rename(id: string, event: Event): void {
    const name = (event.target as HTMLInputElement).value.trim();
    if (name) {
      this.store.update(id, { name });
    }
  }

  protected setInfillPercent(id: string, pct: number): void {
    this.updateParams(id, { infill_density: Math.max(0, Math.min(100, pct)) / 100 });
  }

  protected setQuality(id: string, value: string): void {
    this.store.update(id, { quality: value as PrintQuality });
  }

  protected setPattern(id: string, value: string): void {
    this.updateParams(id, { infill_pattern: value as InfillPattern });
  }

  protected setSeam(id: string, value: string): void {
    this.updateParams(id, { seam_position: value as SeamPosition });
  }

  protected setAdhesion(id: string, value: string): void {
    this.updateParams(id, { adhesion_type: value as AdhesionType });
  }
}

/** Case-insensitive sort by display name (non-mutating). */
function sortByName(items: readonly PrintProfile[]): PrintProfile[] {
  return [...items].sort((a, b) => a.name.localeCompare(b.name));
}
