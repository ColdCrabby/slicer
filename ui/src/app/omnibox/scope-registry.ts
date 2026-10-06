/**
 * The omnibox's scope registry: one list of the app's searchable corners, each
 * with the words that engage it and the provider that builds its results.
 *
 * **Data-driven on purpose.** Adding a scope is adding an entry here — tokens,
 * icon, provider — not a new branch through the palette. The palette reads
 * this list for its suggestions, its chips and its empty state, so what the
 * user can Tab into and what the providers serve can never disagree.
 *
 * Providers are built lazy on purpose: the registry module loads with the
 * palette's chunk (already deferred), but the heavy corpora — the parameter
 * schema, the profile stores, the library and its wasm glue — are only pulled
 * when the palette actually opens, and the library's only when its scope is
 * asked for.
 */
import { type Injector } from '@angular/core';
import type { SlicingParams } from '../../generated/slicer-engine-ws-client-message-v1';
import {
  SETTING_CONTRACTS,
  contractForGroup,
  type SettingContractId,
} from '../models/setting-contract';
import {
  preferenceEntries,
  profileEntries,
  sectionEntries,
  settingEntries,
  type SearchEntry,
} from '../pages/settings/settings-search';
import { ActivePresets } from '../services/profiles/active-presets';
import { ActiveSelection } from '../services/profiles/active-selection';
import { PrintersStore } from '../services/profiles/printers-store';
import { FilamentsStore } from '../services/profiles/filaments-store';
import { PrintProfilesStore } from '../services/profiles/print-profiles-store';
import { Slicer } from '../services/slicer';
import { WorkplateNames } from '../services/workplate-names';
import { WorkplateSettingsStore } from '../services/workplate-settings';
import type { OmniboxEntry } from './omnibox-entries';
import type { ScopeDef } from './scope-tokens';

/** One scope: what it is called, how it is engaged, and what it serves. */
export interface OmniboxScope {
  readonly def: ScopeDef;
  /**
   * Build this scope's entries. Runs when the palette opens — cheap after the
   * first call, because every dynamic import resolves to its loaded module.
   */
  readonly provide: (injector: Injector) => OmniboxEntry[] | Promise<OmniboxEntry[]>;
}

/** A `SearchEntry` reborn as a palette result under one scope. */
function asEntry(entry: SearchEntry, scopeId: string, rank: number): OmniboxEntry {
  return {
    id: `${scopeId}:${entry.id}`,
    scopeId,
    kind: 'navigate',
    title: entry.title,
    where: entry.where,
    icon: entry.icon,
    keywords: entry.keywords,
    rank,
    path: entry.path,
    queryParams: entry.queryParams,
    target: entry.target,
    // Navigation is the one act that takes the user away from what they were
    // looking at, so its row must say so (finding: "go somewhere" vs "change
    // this") and it must not out-rank a result that edits the current view.
    leavesView: true,
  };
}

/** Schema keys the engine re-stamps from the chosen profiles on every slice. */
const IDENTITY_KEYS: ReadonlySet<string> = new Set([
  'filament_type',
  'filament_name',
  'filament_color',
  'printer_vendor',
  'printer_model',
]);

/**
 * The parameters of the plate on screen, as quick-set entries.
 *
 * Shared by the `print` scope and the `workbench` scope — the difference
 * between them is what *else* sits beside the parameters, not the parameters.
 * With no plate open there is nowhere for an override to land, so the same
 * settings come back as navigation to the profile editor that owns them.
 */
async function providePlateParams(injector: Injector, scopeId: string): Promise<OmniboxEntry[]> {
  const { ALL_PARAM_GROUPS } = await import('../components/profiles/profile-param-groups');
  const slicer = injector.get(Slicer);
  const plate = slicer.currentRequestUuid();
  if (!plate) {
    const { EDITOR_PARAM_GROUPS } = await import('../components/profiles/profile-param-groups');
    return settingEntries(EDITOR_PARAM_GROUPS).map((entry) => asEntry(entry, scopeId, 0));
  }
  const names = injector.get(WorkplateNames);
  const where = `This plate · ${names.displayNameFor(plate, null)}`;
  return ALL_PARAM_GROUPS.flatMap((group) => group.fields)
    .filter(
      (field) =>
        // Fan curves and pause triggers have editors of their own; a
        // number box would take their place and mangle them.
        field.type !== 'array' &&
        // The engine re-stamps these from the profiles on every slice, so
        // a change here would be accepted and then quietly dropped.
        !IDENTITY_KEYS.has(field.key),
    )
    .map((field): OmniboxEntry => {
      const contract = SETTING_CONTRACTS.find((c) => c.id === contractForGroup(field.group ?? ''));
      return {
        id: `${scopeId}:${field.key}`,
        scopeId,
        kind: 'quickset',
        title: field.title ?? field.key,
        where,
        icon: 'control-slider',
        keywords: `${field.key.replaceAll('_', ' ')} ${field.description ?? ''}`,
        description: field.description,
        rank: 4,
        // The current plate is exactly what "print settings" edits, so this
        // is the scope the current-view bonus is named for.
        currentView: true,
        field,
        fieldKey: field.key,
        apply: (value) => slicer.updateSettings({ [field.key]: value } as Partial<SlicingParams>),
        openInEditor: contract
          ? { path: contract.managePath, queryParams: { focus: field.key } }
          : undefined,
      };
    });
}

/**
 * The presets the current plate would switch to, as commands.
 *
 * Switching a printer, filament or process is a *current-view* act — it
 * re-pins the stack the open plate slices against — which is why these live
 * in the workbench scope beside the plate's own parameters rather than only
 * behind the profiles' settings pages.
 */
function provideWorkbenchPresets(injector: Injector): OmniboxEntry[] {
  const selection = injector.get(ActiveSelection);
  const active = injector.get(ActivePresets);
  const configs: {
    contract: SettingContractId;
    icon: string;
    label: string;
    select: (id: string) => void;
  }[] = [
    {
      contract: 'printer',
      icon: 'printer',
      label: 'Printer',
      select: (id) => selection.selectPrinter(id),
    },
    {
      contract: 'filament',
      icon: 'droplet',
      label: 'Filament',
      select: (id) => selection.selectFilament(id),
    },
    {
      contract: 'process',
      icon: 'reports',
      label: 'Process',
      select: (id) => selection.selectProfile(id),
    },
  ];
  return configs.flatMap((config) =>
    active.options(config.contract).map((option): OmniboxEntry => {
      const isActive = active.selectedId(config.contract) === option.value;
      return {
        id: `workbench:${config.contract}:${option.value}`,
        scopeId: 'workbench',
        kind: 'command',
        title: option.label,
        where: isActive ? `${config.label} · active` : `Switch ${config.label.toLowerCase()}`,
        icon: config.icon,
        keywords: `${config.contract} ${config.label.toLowerCase()} preset profile switch`,
        rank: 8,
        currentView: true,
        run: () => config.select(option.value),
      };
    }),
  );
}

/**
 * The scopes, in the order their suggestions appear. Registry order is also
 * what breaks a prefix tie — `p` means Print settings because it is listed
 * before Printers, `s` means Settings because it sits above Filaments' `spool`
 * — so a scope that should win a shared prefix must sit above its rival.
 */
export const OMNIBOX_SCOPES: readonly OmniboxScope[] = [
  {
    def: {
      id: 'print',
      label: 'Print settings',
      icon: 'control-slider',
      tokens: ['print', 'params', 'parameters'],
      hint: 'Change a parameter on the current plate',
    },
    provide: (injector) => providePlateParams(injector, 'print'),
  },
  {
    def: {
      id: 'workbench',
      label: 'Workbench',
      icon: 'tools',
      tokens: ['workbench', 'plate', 'current'],
      hint: 'Switch presets and edit this plate',
    },
    // The open plate's whole working set, in one scope: which printer,
    // filament and process it slices with, *and* the individual parameters
    // those stack into — so a preset swap and a one-off tweak are one search
    // apart instead of a trip to three settings pages.
    provide: async (injector) => [
      ...provideWorkbenchPresets(injector),
      ...(await providePlateParams(injector, 'workbench')),
    ],
  },
  {
    def: {
      id: 'printers',
      label: 'Printers',
      icon: 'printer',
      tokens: ['printer', 'printers', 'machine'],
      hint: 'Your printer profiles',
    },
    provide: (injector) =>
      profileEntries({
        printer: injector.get(PrintersStore).items(),
        filament: [],
        process: [],
      })
        .filter((entry) => entry.id.startsWith('profile:printer:'))
        .map((entry) => asEntry(entry, 'printers', 12)),
  },
  {
    def: {
      id: 'settings',
      label: 'Settings',
      icon: 'settings',
      tokens: ['settings', 'preferences', 'prefs', 'app'],
      hint: 'Pages and app preferences',
    },
    provide: () => [
      ...sectionEntries().map((entry) => asEntry(entry, 'settings', 40)),
      ...preferenceEntries().map((entry) => asEntry(entry, 'settings', 15)),
    ],
  },
  {
    def: {
      id: 'filaments',
      label: 'Filaments',
      icon: 'droplet',
      tokens: ['filament', 'filaments', 'material', 'spool'],
      hint: 'Your filament profiles',
    },
    provide: (injector) =>
      profileEntries({
        printer: [],
        filament: injector.get(FilamentsStore).items(),
        process: [],
      })
        .filter((entry) => entry.id.startsWith('profile:filament:'))
        .map((entry) => asEntry(entry, 'filaments', 12)),
  },
  {
    def: {
      id: 'processes',
      label: 'Processes',
      icon: 'reports',
      tokens: ['process', 'processes', 'preset', 'presets', 'profile', 'profiles'],
      hint: 'Your process presets',
    },
    provide: (injector) =>
      profileEntries({
        printer: [],
        filament: [],
        process: injector.get(PrintProfilesStore).items(),
      })
        .filter((entry) => entry.id.startsWith('profile:process:'))
        .map((entry) => asEntry(entry, 'processes', 12)),
  },
  {
    def: {
      id: 'models',
      label: 'Models',
      icon: 'box-iso',
      tokens: ['model', 'models', 'files', 'library', 'lib'],
      hint: 'Put a library model on a plate',
    },
    provide: async (injector) => {
      const { ObjectLibrary } = await import('../services/library/object-library');
      const library = injector.get(ObjectLibrary);
      if (!library.loaded()) {
        void library.refresh();
      }
      const { LibraryActions } = await import('../services/library/library-actions');
      const actions = injector.get(LibraryActions);
      return library.entries().map((entry) => ({
        id: `models:${entry.id}`,
        scopeId: 'models',
        kind: 'command' as const,
        title: entry.name,
        where: 'Library',
        icon: 'box-iso',
        imageId: entry.id,
        thumbnail: () => library.thumbnails().get(entry.id) ?? null,
        ensureThumbnail: () => library.ensureThumbnail(entry),
        keywords: 'model stl obj 3mf file library',
        rank: 12,
        // Adding a model lands it on the plate you are looking at, so it is a
        // current-view action, not a trip to another page.
        currentView: true,
        run: () => void actions.addToPlate(entry),
      }));
    },
  },
];

/** The scope defs alone — what token matching and the chips row need. */
export function omniboxScopeDefs(): readonly ScopeDef[] {
  return OMNIBOX_SCOPES.map((scope) => scope.def);
}

/**
 * Build every scope's entries, ready to search.
 *
 * Providers may be async; failures are isolated to their scope, so a library
 * that will not read leaves the other five working.
 */
export async function collectOmniboxEntries(injector: Injector): Promise<OmniboxEntry[]> {
  const settled = await Promise.all(
    OMNIBOX_SCOPES.map(async (scope) => {
      try {
        return await scope.provide(injector);
      } catch {
        return [];
      }
    }),
  );
  return settled.flat();
}
