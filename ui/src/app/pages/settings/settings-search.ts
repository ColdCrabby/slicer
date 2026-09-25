/**
 * The Settings search: one box in the sidebar that finds a page, an app
 * preference, one of your profiles, or any slicing parameter the profile
 * editors show — and takes you to it.
 *
 * Settings grew past the point where the sidebar alone could say where
 * something lives: preferences moved between pages, and a printer editor holds
 * sixty settings behind one nav entry. Search is what makes that affordable, the
 * same judgement the slice sidebar's parameter search makes (see
 * `progressive-disclosure.instructions.md`, "Search is the real escape hatch").
 *
 * Everything here is data and matching; the shell renders the results and
 * navigates.
 */
import type { SchemaGroup } from '../../schema-form/models/field-def';
import { SETTING_CONTRACTS, type SettingContractId } from '../../models/setting-contract';
import { PREFS, prefAnchor } from './prefs/pref-registry';
import { SETTINGS_GROUPS, sectionLabel } from './settings-sections';

export type SearchKind = 'section' | 'preference' | 'profile' | 'setting';

export interface SearchEntry {
  kind: SearchKind;
  /** Unique across the index; tracks the result row. */
  id: string;
  title: string;
  /** Where it lives — the result's second line, e.g. "3D View · Look". */
  where: string;
  icon: string;
  /** Absolute router path. */
  path: string;
  queryParams?: Record<string, string>;
  /** CSS selector of the element to land on once the page is showing. */
  target?: string;
  /** Matched but never shown. */
  keywords?: string;
}

/** How many results the sidebar lists; past this a query is too vague to be served by scrolling. */
export const MAX_RESULTS = 40;

/** The pages themselves. */
export function sectionEntries(): SearchEntry[] {
  return SETTINGS_GROUPS.flatMap((group) =>
    group.sections.map((section) => ({
      kind: 'section' as const,
      id: `section:${section.path}`,
      title: section.label,
      where: group.title ? `${group.title} settings` : 'Settings',
      icon: section.icon,
      path: `/settings/${section.path}`,
      keywords: section.keywords,
    })),
  );
}

/** Every app preference, from the same registry the pages render from. */
export function preferenceEntries(): SearchEntry[] {
  return PREFS.map((pref) => ({
    kind: 'preference' as const,
    id: `pref:${pref.id}`,
    title: pref.title,
    where: `${sectionLabel(pref.page)} · ${pref.group}`,
    icon: 'settings',
    path: `/settings/${pref.page}`,
    target: `#${prefAnchor(pref.id)}`,
    keywords: [pref.hint, pref.detail, pref.keywords].filter(Boolean).join(' '),
  }));
}

/** One of the user's profiles, as the shell reads it from a store. */
export interface ProfileRef {
  id: string;
  name: string;
}

const PROFILE_PAGES: Record<SettingContractId, { path: string; label: string; icon: string }> = {
  printer: { path: 'printers', label: 'Printers', icon: 'printer' },
  filament: { path: 'filaments', label: 'Filaments', icon: 'droplet' },
  process: { path: 'profiles', label: 'Processes', icon: 'reports' },
};

/** The user's own printers, filaments and processes, by name. */
export function profileEntries(profiles: Record<SettingContractId, readonly ProfileRef[]>) {
  return (Object.keys(PROFILE_PAGES) as SettingContractId[]).flatMap((kind) =>
    profiles[kind].map((profile): SearchEntry => ({
      kind: 'profile',
      id: `profile:${kind}:${profile.id}`,
      title: profile.name,
      where: PROFILE_PAGES[kind].label,
      icon: PROFILE_PAGES[kind].icon,
      path: `/settings/${PROFILE_PAGES[kind].path}`,
      queryParams: { id: profile.id },
    })),
  );
}

/**
 * Every slicing parameter the profile editors render, and the hand-built
 * sections beside them that people look for by name.
 *
 * Opens the editor on its default profile and lands on the field, which is the
 * editor's own `focus` hand-off — the one the field notices already use.
 */
export function settingEntries(groups: Record<SettingContractId, readonly SchemaGroup[]>) {
  const entries: SearchEntry[] = [];
  for (const contract of SETTING_CONTRACTS) {
    const page = PROFILE_PAGES[contract.id];
    for (const group of groups[contract.id]) {
      for (const field of group.fields) {
        entries.push({
          kind: 'setting',
          id: `setting:${contract.id}:${field.key}`,
          title: field.title ?? field.key,
          where: `${page.label} · ${group.name}`,
          icon: page.icon,
          path: contract.managePath,
          queryParams: { focus: field.key },
          // The description is the corpus the schema writes for exactly this —
          // it carries the names other slicers use ("retraction length" for
          // what this one calls Retraction Distance). Matched, never shown, and
          // outranked by a title hit, so a word it merely mentions sorts last.
          keywords: `${field.key.replaceAll('_', ' ')} ${field.description ?? ''}`,
        });
      }
    }
  }
  // The printer editor's hand-written sections — no schema field stands for them.
  const printerSections: [string, string, string][] = [
    ['Connection', 'connection', 'host ip address api key moonraker octoprint network'],
    ['Build volume', 'build-volume', 'bed size shape width depth height origin'],
    ['G-code', 'gcode', 'start end layer change custom gcode template macro'],
    ['Material corrections', 'corrections', 'per material override calibration flow'],
  ];
  for (const [title, focus, keywords] of printerSections) {
    entries.push({
      kind: 'setting',
      id: `setting:printer:@${focus}`,
      title,
      where: 'Printers',
      icon: 'printer',
      path: '/settings/printers',
      queryParams: { focus },
      keywords,
    });
  }
  return entries;
}

/** How a result ranks. Higher first. */
function score(entry: SearchEntry, query: string, words: readonly string[]): number {
  const title = entry.title.toLowerCase();
  const haystack = `${title} ${entry.where.toLowerCase()} ${(entry.keywords ?? '').toLowerCase()}`;
  if (!words.every((word) => haystack.includes(word))) {
    return 0;
  }
  let points = 1;
  if (title === query) {
    points += 200;
  } else if (title.startsWith(query)) {
    points += 120;
  } else if (title.includes(query)) {
    points += 60;
  }
  const titleWords = title.split(/[^a-z0-9°]+/);
  for (const word of words) {
    if (titleWords.some((t) => t.startsWith(word))) {
      points += 20;
    } else if (title.includes(word)) {
      points += 8;
    }
  }
  // A page and a named preference outrank the hundreds of parameters that can
  // mention the same word, and your own profile outranks a parameter too.
  points += { section: 40, preference: 15, profile: 10, setting: 0 }[entry.kind];
  return points;
}

/**
 * The entries matching `query`, best first.
 *
 * Every word must appear — in the title, where it lives, or its keywords — so
 * "bed temp" finds Bed Temperature and not every setting that mentions a bed.
 * Plain substring matching, deliberately: a fuzzy match is kind to typos and
 * unkind to a list of two hundred similar names, where it surfaces confident
 * nonsense.
 */
export function searchSettings(
  entries: readonly SearchEntry[],
  query: string,
  limit = MAX_RESULTS,
): SearchEntry[] {
  const needle = query.trim().toLowerCase();
  if (!needle) {
    return [];
  }
  const words = needle.split(/\s+/);
  return entries
    .map((entry) => ({ entry, points: score(entry, needle, words) }))
    .filter((hit) => hit.points > 0)
    .sort((a, b) => b.points - a.points || a.entry.title.length - b.entry.title.length)
    .slice(0, limit)
    .map((hit) => hit.entry);
}
