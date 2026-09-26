/**
 * The pages of Settings, in the groups the sidebar shows them in.
 *
 * One list, read by the sidebar and by the Settings search, so a page that is
 * in one is in the other.
 */
export interface SettingsSection {
  /** Route under `/settings`. */
  path: string;
  label: string;
  icon: string;
  /** Other words for the page, for search only. */
  keywords: string;
}

export interface SettingsSectionGroup {
  id: string;
  /** Heading above the group; `null` for one that needs none. */
  title: string | null;
  sections: readonly SettingsSection[];
}

/**
 * Two kinds of thing live in Settings and the grouping says which is which:
 * **App** — how this app behaves on this device — and **Library** — the
 * printers, filaments and processes you slice with, which travel with the
 * engine. The release notes and the destructive resets sit apart at the foot,
 * where nobody lands on them by accident.
 */
export const SETTINGS_GROUPS: readonly SettingsSectionGroup[] = [
  {
    id: 'app',
    title: 'App',
    sections: [
      {
        path: 'general',
        label: 'General',
        icon: 'control-slider',
        keywords: 'slicing auto slice library export backup about version',
      },
      {
        path: 'appearance',
        label: 'Appearance',
        icon: 'palette',
        keywords: 'theme dark light mode accent colour color',
      },
      {
        path: '3d-view',
        label: '3D View',
        icon: 'box-3d-point',
        keywords: 'viewer render graphics shadows camera thumbnail performance',
      },
      {
        path: 'controls',
        label: 'Controls',
        icon: 'mouse-button-left',
        keywords: 'gestures trackpad touch pencil stylus keyboard shortcuts hotkeys',
      },
    ],
  },
  {
    id: 'library',
    title: 'Library',
    sections: [
      {
        path: 'printers',
        label: 'Printers',
        icon: 'printer',
        keywords: 'machine bed nozzle firmware connection gcode',
      },
      {
        path: 'filaments',
        label: 'Filaments',
        icon: 'droplet',
        keywords: 'material spool temperature pla petg abs',
      },
      {
        path: 'profiles',
        label: 'Processes',
        icon: 'reports',
        keywords: 'print profiles process presets quality layer walls infill',
      },
      { path: 'labels', label: 'Labels', icon: 'label', keywords: 'tags organise organize' },
    ],
  },
  {
    id: 'more',
    title: null,
    sections: [
      {
        path: 'changelog',
        label: "What's New",
        icon: 'sparks',
        keywords: 'changelog release notes version update',
      },
      {
        path: 'danger-zone',
        label: 'Danger Zone',
        icon: 'warning-triangle',
        keywords: 'reset clear history factory delete erase',
      },
    ],
  },
];

/** Every section, in sidebar order. */
export const SETTINGS_SECTIONS: readonly SettingsSection[] = SETTINGS_GROUPS.flatMap(
  (group) => group.sections,
);

/** A section's label by its path — for the search's "where" line. */
export function sectionLabel(path: string): string {
  return SETTINGS_SECTIONS.find((section) => section.path === path)?.label ?? path;
}
