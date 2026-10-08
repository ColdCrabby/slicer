/**
 * The omnibox's result model: one entry with the scope it belongs to, the
 * search that narrows to the locked scopes, and the ranking that orders what
 * is left.
 *
 * The ranking deliberately mirrors the Settings search's (`settings-search.ts`)
 * — every word must appear, a title hit outranks a keyword hit, and named
 * things outrank the hundreds of parameters that can merely mention them — so
 * a user who learned what the settings box does finds the palette agrees.
 * What the palette adds is the scope: each entry knows where it lives, which
 * is what a lock filters by, and each scope's provider stamps its own
 * prominence so pages can outrank parameters without this file knowing which
 * is which.
 */
import type { FieldDef } from '../schema-form/models/field-def';
import { toDisplay } from '../schema-form/models/field-units';
import type { ScopeDef } from './scope-tokens';

/** What acting on an entry does — the palette is a searcher, not a form. */
export type OmniboxEntryKind = 'navigate' | 'command' | 'quickset';

export interface OmniboxEntry {
  /** Unique across the whole palette; tracks the result row. */
  readonly id: string;
  /** Id of the {@link ScopeDef} this entry belongs to; what a lock filters by. */
  readonly scopeId: string;
  readonly kind: OmniboxEntryKind;
  readonly title: string;
  /** Where it lives — the result's second line, e.g. "Printers · Build volume". */
  readonly where: string;
  readonly icon: string;
  /** Matched but never shown; the corpus the schema and stores write for this. */
  readonly keywords?: string;
  /** The scope provider's say in how high this result floats. */
  readonly rank?: number;
  /**
   * Markdown blurb shown under the title where a description is expected — the
   * schema's own `description`, unaltered. A row renders it as Markdown.
   */
  readonly description?: string;
  /** Library entry whose thumbnail the row should show, if any. */
  readonly imageId?: string;
  /**
   * The row's thumbnail right now, if one is ready. A closure over the
   * library's signal map rather than a baked URL, so a thumbnail that lands
   * while the palette is open flips the row when it arrives — and the library
   * module itself stays out of the palette until a scope that needs it loads.
   */
  readonly thumbnail?: () => string | null;
  /**
   * Kicks thumbnail production for the entry — fetching it if the library has
   * one, rendering it if it does not. Called for the row the user is pointed
   * at, not for every result: rendering a whole library of previews because a
   * query mentioned one of them is work nobody asked for.
   */
  readonly ensureThumbnail?: () => void;

  /**
   * `true` when acting on this changes *the view you are looking at* (the
   * current plate, the open editor); absent or `false` means it is global —
   * it goes somewhere else or binds app-wide. The ranking floats current-view
   * results above global ones when both match equally.
   */
  readonly currentView?: boolean;
  /**
   * `navigate` only: `true` when the action leaves for a different route
   * rather than editing what is already on screen. The row shows a popout
   * glyph so "go somewhere" never reads like "change this".
   */
  readonly leavesView?: boolean;

  /** `navigate`: absolute router path, plus its query and an anchor selector. */
  readonly path?: string;
  readonly queryParams?: Record<string, string>;
  readonly target?: string;

  /** `quickset`: the parsed schema field to edit, keyed how the engine spells it. */
  readonly field?: FieldDef;
  readonly fieldKey?: string;
  /** `quickset`: applies a new stored value to the current workplate. */
  readonly apply?: (value: unknown) => void;
  /** `quickset`: where the same setting lives in the profile editors. */
  readonly openInEditor?: { path: string; queryParams?: Record<string, string> };

  /** `command`: runs the action; the palette closes around it. */
  readonly run?: () => void | Promise<void>;
}

export const OMNIBOX_MAX_RESULTS = 30;

/**
 * The head start a result that acts on the current view gets over a global one.
 *
 * Kept below the weakest title bonus (a title word hit, +20 at least) so it can
 * only break ties between otherwise comparable matches: a clearly better global
 * hit still outranks a weak current-view one. It exists because "the setting I
 * am looking at" is what a user means more often than "the same-named page
 * three routes away", and the palette should agree without hiding the other.
 */
export const OMNIBOX_CURRENT_VIEW_BONUS = 10;

/**
 * Keep only what the locked scopes cover. No locks means everywhere, and a
 * lock means *exclusively there* — filament entries behind a Settings lock
 * are not ranked lower, they are gone.
 */
export function narrowToScopes<T extends { scopeId: string }>(
  entries: readonly T[],
  locked: readonly string[],
): T[] {
  if (locked.length === 0) {
    return [...entries];
  }
  return entries.filter((entry) => locked.includes(entry.scopeId));
}

/** How a result ranks. Higher first; the shape is the Settings search's. */
function score(entry: OmniboxEntry, query: string, words: readonly string[]): number {
  const title = entry.title.toLowerCase();
  const haystack = `${title} ${entry.where.toLowerCase()} ${(entry.keywords ?? '').toLowerCase()}`;
  if (!words.every((word) => haystack.includes(word))) {
    return 0;
  }
  let points = 1;
  if (entry.currentView) {
    points += OMNIBOX_CURRENT_VIEW_BONUS;
  }
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
  return points + (entry.rank ?? 0);
}

/**
 * The entries matching `query`, best first, from the scopes in play.
 *
 * Plain substring matching, as the Settings search argues: a fuzzy match is
 * kind to typos and unkind to a list of two hundred similar names, where it
 * surfaces confident nonsense.
 */
export function searchOmniboxEntries(
  entries: readonly OmniboxEntry[],
  query: string,
  locked: readonly string[] = [],
  limit = OMNIBOX_MAX_RESULTS,
): OmniboxEntry[] {
  const needle = query.trim().toLowerCase();
  if (!needle) {
    return [];
  }
  const words = needle.split(/\s+/);
  return narrowToScopes(entries, locked)
    .map((entry) => ({ entry, points: score(entry, needle, words) }))
    .filter((hit) => hit.points > 0)
    .sort((a, b) => b.points - a.points || a.entry.title.length - b.entry.title.length)
    .slice(0, limit)
    .map((hit) => hit.entry);
}

/**
 * A locked scope with an empty query shows its wares instead of a search:
 * the top of the scope by prominence, so `sett` + Tab is already a menu of
 * the Settings pages before a word is typed.
 */
export function browseScope(
  entries: readonly OmniboxEntry[],
  locked: readonly string[],
  limit = OMNIBOX_MAX_RESULTS,
): OmniboxEntry[] {
  return narrowToScopes(entries, locked)
    .sort(
      (a, b) =>
        (b.rank ?? 0) - (a.rank ?? 0) || a.title.localeCompare(b.title) || a.id.localeCompare(b.id),
    )
    .slice(0, limit);
}

/**
 * The stored value as a short string for a result's right edge — `42 %`, not
 * a raw `0.42`. Units follow the field's `x-unit` the same way every field
 * control displays them; enums use their option labels, and anything without
 * a shape falls back to the raw value.
 */
export function describeValue(field: FieldDef | undefined, value: unknown): string {
  if (value === undefined || value === null || value === '') {
    return '—';
  }
  if (typeof value === 'boolean') {
    return value ? 'On' : 'Off';
  }
  if (field?.enumOptions) {
    return (
      field.enumOptions.find((option) => `${option.value}` === `${value}`)?.label ?? `${value}`
    );
  }
  if (typeof value === 'number') {
    if (field?.unit === 'fraction') {
      return `${round(toDisplay(value, 100))} %`;
    }
    if (field?.unit === 'percent') {
      return `${round(value)} %`;
    }
    if (field?.unit === 'ratio') {
      return `${round(value)} ×`;
    }
    return round(value);
  }
  return `${value}`;
}

function round(value: number): string {
  return `${Math.round(value * 100) / 100}`;
}
