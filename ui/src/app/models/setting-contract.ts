/**
 * Slicer settings are split across three *contracts* — the profile domains that
 * established slicers (PrusaSlicer, OrcaSlicer) present as top-level tabs:
 *
 * - **Printer** — machine hardware, firmware/output, and extruder behaviour.
 * - **Filament** — material temperatures and cooling.
 * - **Process** — the print/slice parameters a print profile presets.
 *
 * Every slicer parameter already carries an `x-group` (Layer, Walls, Infill, …)
 * for fine-grained accordion grouping. This module maps each of those groups to
 * the contract that *owns* it, so the sidebar can categorise the flat parameter
 * list the same way a print profile, filament, or printer preset would.
 *
 * A print profile is nothing more than a saved set of Process parameters, which
 * is why the Process contract collects the bulk of the groups.
 */
export type SettingContractId = 'printer' | 'filament' | 'process';

export interface SettingContract {
  id: SettingContractId;
  /** Tab label. */
  label: string;
  /** Iconoir icon name (shared with the Settings sub-nav). */
  icon: string;
  /** Settings route managing this contract's presets. */
  managePath: string;
  /** `x-group` names owned by this contract, in display order. */
  groups: string[];
}

/**
 * Contract catalog, in tab order. Group assignments follow the conventions of
 * established slicers: retraction/output live with the *printer* (extruder +
 * firmware), temperature/cooling live with the *filament*, and everything that
 * shapes the print itself is a *process* parameter.
 */
export const SETTING_CONTRACTS: readonly SettingContract[] = [
  {
    id: 'printer',
    label: 'Printer',
    icon: 'printer',
    managePath: '/settings/printers',
    groups: ['Hardware', 'Retraction', 'Output', 'Time estimate'],
  },
  {
    id: 'filament',
    label: 'Filament',
    icon: 'droplet',
    managePath: '/settings/filaments',
    groups: ['Temperature', 'Cooling', 'Filament G-code'],
  },
  {
    id: 'process',
    label: 'Process',
    icon: 'reports',
    managePath: '/settings/profiles',
    groups: [
      'Layer',
      'Walls',
      'Extrusion',
      'Infill',
      'Support',
      'Speed',
      'Quality',
      'Dimensions',
      'Surfaces',
      'Adhesion',
      'Objects',
      'Thumbnail',
      'Mesh',
    ],
  },
];

/**
 * Icon (iconoir name) for each `x-group`, so every collapsible section in the
 * slice sidebar has a visual anchor. Keyed by group name.
 */
export const GROUP_ICONS: Record<string, string> = {
  Layer: 'multiple-pages',
  Walls: 'frame',
  Infill: 'grid-plus',
  Speed: 'dashboard-speed',
  Quality: 'medal',
  Dimensions: 'ruler-combine',
  Surfaces: 'fill-color',
  Mesh: 'box-iso',
  Temperature: 'temperature-high',
  Cooling: 'snow-flake',
  Hardware: 'wrench',
  Retraction: 'undo',
  Output: 'code-brackets',
  'Time estimate': 'timer',
  'Filament G-code': 'code-brackets',
  Extrusion: 'extrude',
  Support: 'view-structure-down',
  Adhesion: 'magnet-energy',
  Objects: 'packages',
  Thumbnail: 'media-image',
};

/** The contract that owns a given `x-group`; unmapped groups fall to Process. */
export function contractForGroup(group: string): SettingContractId {
  for (const contract of SETTING_CONTRACTS) {
    if (contract.groups.includes(group)) {
      return contract.id;
    }
  }
  return 'process';
}

/**
 * Bucket the group names actually present in a schema into their owning
 * contracts. Known groups keep their taxonomy order; any group not claimed by
 * a contract is appended to Process so new parameters are never hidden.
 */
export function bucketGroupsByContract(
  groupNames: readonly string[],
): Record<SettingContractId, string[]> {
  const present = new Set(groupNames);
  const buckets: Record<SettingContractId, string[]> = {
    printer: [],
    filament: [],
    process: [],
  };

  for (const contract of SETTING_CONTRACTS) {
    for (const group of contract.groups) {
      if (present.has(group)) {
        buckets[contract.id].push(group);
      }
    }
  }

  for (const group of groupNames) {
    if (contractForGroup(group) === 'process' && !buckets.process.includes(group)) {
      buckets.process.push(group);
    }
  }

  return buckets;
}
