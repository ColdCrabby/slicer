/**
 * A table of contents for a profile editor, read off the page it describes.
 *
 * The printer, filament and print-profile editors are the one place that shows
 * **every** parameter — no accordion, no tiers, nothing folded away. That is
 * what they are for, and it is also what makes them hard: a single column of
 * two hundred controls that can only be navigated by scrolling, with no way to
 * search them at all.
 *
 * The outline is the map. It is derived from the rendered DOM rather than from
 * the schema, for one reason: a schema-built outline would list the
 * schema-driven sections and silently miss Identity, Connection, Build volume
 * and G-code — hand-written blocks that hold some of the settings people look
 * for most. A contents list built from the headings on the page cannot drift
 * from the page.
 *
 * The two hooks it reads are the project's own shared primitives, each defined
 * in exactly one place: `profile-editor__group-title` labels a section, and
 * `field-shell__title` / `field-row__title` label a row.
 */

/**
 * Where a row's target sits in the editor's scrollable content, in px from the
 * top of it. Measured once per scan so scrolling only ever compares numbers.
 */
export interface OutlineSpan {
  top: number;
  bottom: number;
}

/** One setting in the outline. */
export interface OutlineEntry {
  /** Unique within the outline; identifies the row across a rescan. */
  id: string;
  title: string;
  /** What a jump scrolls to. */
  el: HTMLElement;
}

/** One section of the editor, with the settings inside it. */
export interface OutlineSection {
  id: string;
  title: string;
  el: HTMLElement;
  entries: OutlineEntry[];
}

const SECTION_SELECTOR = '.profile-editor__group';
const SECTION_TITLE_SELECTOR = '.profile-editor__group-title';
const ENTRY_TITLE_SELECTOR = '.field-shell__title, .field-row__title';

/** The element a jump should scroll to for a given row title. */
function rowFor(title: HTMLElement): HTMLElement {
  return title.closest<HTMLElement>('nexus-field-shell, nexus-field-row') ?? title;
}

/** Read the outline of whatever `root` currently renders. */
/**
 * Measure every row against the scroller's content box.
 *
 * Taken in one pass, right after a scan, so the per-frame question "what is on
 * screen" is a pair of number comparisons rather than a `getBoundingClientRect`
 * per row — on a print profile that is two hundred rects every frame of a
 * scroll, which is exactly the cost a contents list may not add.
 */
export function measureOutline(
  sections: readonly OutlineSection[],
  scroller: HTMLElement,
): Map<string, OutlineSpan> {
  const spans = new Map<string, OutlineSpan>();
  const origin = scroller.getBoundingClientRect().top - scroller.scrollTop;
  const put = (id: string, el: HTMLElement) => {
    const rect = el.getBoundingClientRect();
    spans.set(id, { top: rect.top - origin, bottom: rect.bottom - origin });
  };
  for (const section of sections) {
    put(section.id, section.el);
    for (const entry of section.entries) {
      put(entry.id, entry.el);
    }
  }
  return spans;
}

/**
 * Ids whose row lies **entirely** inside the window `[top, bottom]`.
 *
 * Fully, not partly: a row clipped by the edge of the editor is one the reader
 * cannot actually read, and counting it made the marked span consistently claim
 * a setting or two more than was on screen — which is the one thing a "what can
 * I see" indicator must not do.
 */
export function idsInView(
  spans: ReadonlyMap<string, OutlineSpan>,
  top: number,
  bottom: number,
): Set<string> {
  const visible = new Set<string>();
  for (const [id, span] of spans) {
    if (span.top >= top && span.bottom <= bottom) {
      visible.add(id);
    }
  }
  return visible;
}

export function scanOutline(root: ParentNode): OutlineSection[] {
  const sections: OutlineSection[] = [];
  const seen = new Set<string>();

  for (const el of Array.from(root.querySelectorAll<HTMLElement>(SECTION_SELECTOR))) {
    const title = el.querySelector<HTMLElement>(SECTION_TITLE_SELECTOR)?.textContent?.trim();
    if (!title) {
      continue;
    }
    const id = unique(title, seen);
    const entries: OutlineEntry[] = [];
    for (const row of Array.from(el.querySelectorAll<HTMLElement>(ENTRY_TITLE_SELECTOR))) {
      const rowTitle = row.textContent?.trim();
      if (!rowTitle) {
        continue;
      }
      entries.push({ id: unique(`${id}/${rowTitle}`, seen), title: rowTitle, el: rowFor(row) });
    }
    sections.push({ id, title, el, entries });
  }

  return sections;
}

/**
 * Make `base` unique among `seen`.
 *
 * Titles repeat across an editor — two sections can both hold a "Name" — and a
 * repeated id would make `@for` track two different rows as one, so the second
 * would scroll to the first.
 */
function unique(base: string, seen: Set<string>): string {
  let id = base;
  let n = 2;
  while (seen.has(id)) {
    id = `${base} (${n++})`;
  }
  seen.add(id);
  return id;
}

/**
 * Narrow an outline to the rows matching `query`, dropping sections left empty.
 *
 * Plain case-insensitive substring matching, deliberately. These pages have no
 * settings search at all today, so this is also the only way to look one up by
 * name — and keeping the matches under their own sections is the point: it says
 * *where* a setting lives, which a flat list of results does not. A section
 * whose own name matches keeps all of its rows.
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
    if (section.title.toLowerCase().includes(needle)) {
      matches.push(section);
      continue;
    }
    const entries = section.entries.filter((entry) => entry.title.toLowerCase().includes(needle));
    if (entries.length > 0) {
      matches.push({ ...section, entries });
    }
  }
  return matches;
}
