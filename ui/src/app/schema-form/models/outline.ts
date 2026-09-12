import { FieldDef, SchemaGroup } from './field-def';
import { Tier, tierOf } from './relevance';

/**
 * A flat, name-only table of contents for a schema form.
 *
 * The accordion answers "what is in this section"; it cannot answer "where does
 * the thing I half-remember live". Search cannot either — it needs the word.
 * The outline is the third question: every section and every setting *name* at
 * once, dense enough to skim, with a jump behind each row.
 *
 * Two rules give it its value, and both are the opposite of what the form does:
 *
 * - **It is never tier-filtered.** An expert setting is listed here even while
 *   the form has it folded away, because not knowing a name is exactly the
 *   situation disclosure leaves the user stranded in. The row says which tier
 *   it sits behind; following it is what reveals it.
 * - **It carries the whole contract**, not the revealed slice of it, so the
 *   count beside a section is the real size of that section.
 */
export interface OutlineEntry {
  /** Schema property key — the jump target. */
  key: string;
  /** Human label, as the form would show it. */
  title: string;
  /** How deep the form keeps this field; `everyday` needs no reveal to reach. */
  tier: Tier;
  /** Whether the value deviates from the baseline the host passed in. */
  modified: boolean;
}

export interface OutlineSection {
  name: string;
  /** Icon name for the section, when its group has one. */
  icon?: string;
  entries: OutlineEntry[];
  /** How many of this section's settings currently deviate from the baseline. */
  modifiedCount: number;
}

/** Build the outline for `groups`, in the order the form lists them. */
export function buildOutline(
  groups: readonly SchemaGroup[],
  icons: Record<string, string>,
  modifiedKeys: ReadonlySet<string>,
): OutlineSection[] {
  return groups.map((group) => {
    const entries = group.fields.map((field) => entryFor(field, modifiedKeys));
    return {
      name: group.name,
      icon: icons[group.name],
      entries,
      modifiedCount: entries.filter((entry) => entry.modified).length,
    };
  });
}

function entryFor(field: FieldDef, modifiedKeys: ReadonlySet<string>): OutlineEntry {
  return {
    key: field.key,
    title: field.title ?? field.key,
    tier: tierOf(field),
    modified: modifiedKeys.has(field.key),
  };
}

/**
 * Narrow an outline to the rows matching `query`, dropping sections left empty.
 *
 * Plain case-insensitive substring matching, deliberately — this is not the
 * fuzzy search beside it. The outline's job is to keep the *shape* of the
 * settings visible while the user narrows it, and a fuzzy match that pulls in
 * near-misses from four other sections destroys the shape it is meant to show.
 * A section whose own name matches keeps all of its entries, so typing a
 * section name gives that section's full contents.
 */
export function filterOutline(
  sections: readonly OutlineSection[],
  query: string,
): OutlineSection[] {
  const needle = query.trim().toLowerCase();
  if (!needle) {
    return [...sections];
  }
  const matches: OutlineSection[] = [];
  for (const section of sections) {
    if (section.name.toLowerCase().includes(needle)) {
      matches.push(section);
      continue;
    }
    const entries = section.entries.filter(
      (entry) =>
        entry.title.toLowerCase().includes(needle) || entry.key.toLowerCase().includes(needle),
    );
    if (entries.length > 0) {
      matches.push({
        ...section,
        entries,
        modifiedCount: entries.filter((entry) => entry.modified).length,
      });
    }
  }
  return matches;
}
