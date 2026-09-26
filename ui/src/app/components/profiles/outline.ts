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

/**
 * What counts as a row.
 *
 * The first two are the shared field primitives, which is why a hand-written
 * block and a schema-driven one both list correctly. `.outline-entry-title` is
 * the opt-in for a heading that is a row but not a field — a material inside the
 * corrections section names a group of settings, not a setting.
 */
const ENTRY_TITLE_SELECTOR = '.field-shell__title, .field-row__title, .outline-entry-title';

/**
 * A subtree whose rows the outline does not list.
 *
 * For a section that is a list of *things* rather than a list of settings: the
 * corrections section is read as "PLA, ABS, PETG", and listing each material's
 * settings under it would bury those three names in a dozen entries — several of
 * them the same words, because two machines' materials correct the same setting.
 */
const SKIP_SELECTOR = '[data-outline-skip]';

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
      if (!rowTitle || row.closest(SKIP_SELECTOR)) {
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

/**
 * The contents rail's narrowest width — `--outline-min-w` in
 * styles/components/_manage.scss. It grows past this only with room to spare.
 */
export const RAIL_WIDTH = 208;

/**
 * The narrowest the editor may be squeezed to before the rail gives up its
 * column: a 448 px form plus its 16 px gutter on each side.
 *
 * 448 is where a setting's label and its control still share one line — the
 * number inputs, the widest control a row holds, keep their full width beside a
 * two-word label. Below it rows start to wrap, and a map of the page is not
 * worth making every row of the page two lines tall. It used to be the editor's
 * full 560 px cap, which asked for more width than an iPad has once the list
 * column is paid for, so the rail was never shown on one.
 */
export const EDITOR_MIN_WIDTH = 448 + 16 * 2;

/**
 * Whether the page can afford the contents rail.
 *
 * The sum is what the grid would do: take the list track and the two gaps off
 * the body, then the rail itself, and see whether the editor still clears
 * {@link EDITOR_MIN_WIDTH}.
 *
 * `bodyWidth` must be measured on an element whose width does not depend on
 * whether the rail is showing — `.mgr__body`, whose size its parent decides —
 * or revealing the rail would take away the room that revealed it, and the
 * answer would oscillate.
 */
export function hasRoomForRail(bodyWidth: number, listWidth: number, gap: number): boolean {
  return bodyWidth - listWidth - gap * 2 - RAIL_WIDTH >= EDITOR_MIN_WIDTH;
}

/**
 * Whether folding the Settings section list is what would make room for the
 * rail.
 *
 * True only when the rail does not fit beside an open section list but does
 * beside a folded one. With room either way there is nothing to fold for; with
 * room neither way, folding would cost the section labels and still not show
 * the outline — a trade with nothing on the other side.
 *
 * Both widths are the body as it *would* measure in each state, so the answer
 * does not change when the section list acts on it — see {@link hasRoomForRail}
 * for why that stability matters.
 */
export function railNeedsFold(
  bodyWhenOpen: number,
  bodyWhenFolded: number,
  listWidth: number,
  gap: number,
): boolean {
  return (
    !hasRoomForRail(bodyWhenOpen, listWidth, gap) && hasRoomForRail(bodyWhenFolded, listWidth, gap)
  );
}

// ── The viewport graph ──────────────────────────────────────────────────────
//
// The rail draws one continuous line down its left edge, the way a git client
// draws a branch: at a section it runs through the section's node, and where
// that section's settings are listed it swings in to their indent and back out
// again below them. On top of it, a thicker stroke marks exactly the stretch of
// the line that corresponds to what the editor is showing — not the rows that
// happen to be fully on screen, but the window itself, mapped pixel for pixel,
// so it slides as the editor scrolls instead of stepping from row to row.
//
// Everything below is plain geometry over numbers the component measures, so it
// is tested without a browser.

/** A row as the rail draws it: where it sits in the rail's content, and how deep. */
export interface RailRow {
  id: string;
  /** 0 for a section, 1 for a setting listed under one. */
  depth: number;
  /** px from the top of the rail's content. */
  top: number;
  bottom: number;
}

export interface GraphPoint {
  x: number;
  y: number;
}

/** A y in the editor's content paired with the y in the rail it lands on. */
export interface RailAnchor {
  from: number;
  to: number;
}

/**
 * The line's x at each depth, in px from the rail's left edge. The stylesheet
 * indents the rows against these through `--outline-lane-*`, which the
 * component writes from this same array so the two cannot drift.
 */
export const GRAPH_LANES: readonly number[] = [7, 19];

/**
 * How much rail height a change of lane is drawn over. Short enough to happen
 * in the seam between two rows, long enough to read as a curve rather than a
 * step.
 */
const GRAPH_BEND = 12;

/** Points per bend. The eased curve is visibly smooth from about six. */
const BEND_SAMPLES = 8;

/** Ease with a vertical tangent at both ends — the shape a branch line takes. */
function smoothstep(t: number): number {
  return t * t * (3 - 2 * t);
}

function middle(row: RailRow): number {
  return (row.top + row.bottom) / 2;
}

function laneOf(depth: number, lanes: readonly number[]): number {
  return lanes[Math.min(Math.max(depth, 0), lanes.length - 1)] ?? 0;
}

/**
 * The line through a list of rows, as a polyline running top to bottom.
 *
 * Within a row the line holds the row's lane. Where two neighbours sit at
 * different depths it bends across the seam between them, centred on it and
 * never reaching past either row's middle, so a short row cannot be skipped.
 * Straight runs cost two points however many rows they pass, which keeps a
 * folded outline of a dozen sections to a handful of points.
 *
 * A section row at either end — the ones drawn with a node — ends the line at
 * its middle, so the line stops in the dot instead of running past it. A
 * setting row at an end has no dot, and the line runs its full height.
 *
 * `y` never decreases along the result — {@link sliceLine} depends on it.
 */
export function graphLine(
  rows: readonly RailRow[],
  lanes: readonly number[] = GRAPH_LANES,
  bend = GRAPH_BEND,
): GraphPoint[] {
  if (rows.length === 0) {
    return [];
  }
  const first = rows[0];
  const points: GraphPoint[] = [
    { x: laneOf(first.depth, lanes), y: first.depth === 0 ? middle(first) : first.top },
  ];
  for (let i = 1; i < rows.length; i++) {
    const prev = rows[i - 1];
    const row = rows[i];
    const x0 = laneOf(prev.depth, lanes);
    const x1 = laneOf(row.depth, lanes);
    if (x0 === x1) {
      continue;
    }
    const seam = (prev.bottom + row.top) / 2;
    const half = Math.max(
      0,
      Math.min(bend / 2, (prev.bottom - prev.top) / 2, (row.bottom - row.top) / 2),
    );
    const y0 = Math.max(seam - half, points[points.length - 1].y);
    const y1 = Math.max(seam + half, y0);
    points.push({ x: x0, y: y0 });
    for (let s = 1; s <= BEND_SAMPLES; s++) {
      const t = s / BEND_SAMPLES;
      points.push({ x: x0 + (x1 - x0) * smoothstep(t), y: y0 + (y1 - y0) * t });
    }
  }
  const last = rows[rows.length - 1];
  points.push({
    x: laneOf(last.depth, lanes),
    y: Math.max(last.depth === 0 ? middle(last) : last.bottom, points[points.length - 1].y),
  });
  return points;
}

/**
 * The stretch of a top-to-bottom polyline between two heights, with its ends
 * interpolated onto the line so the stretch starts and stops exactly there.
 *
 * Empty when the band misses the line or has no height.
 */
export function sliceLine(points: readonly GraphPoint[], from: number, to: number): GraphPoint[] {
  if (points.length < 2 || to <= from) {
    return [];
  }
  const lo = Math.max(from, points[0].y);
  const hi = Math.min(to, points[points.length - 1].y);
  if (hi <= lo) {
    return [];
  }
  const at = (y: number): GraphPoint => {
    for (let i = 1; i < points.length; i++) {
      const a = points[i - 1];
      const b = points[i];
      if (y <= b.y) {
        const t = b.y === a.y ? 1 : (y - a.y) / (b.y - a.y);
        return { x: a.x + (b.x - a.x) * t, y };
      }
    }
    return { ...points[points.length - 1], y };
  };
  const slice: GraphPoint[] = [at(lo)];
  for (const point of points) {
    if (point.y > lo && point.y < hi) {
      slice.push(point);
    }
  }
  slice.push(at(hi));
  return slice;
}

/** A polyline as SVG path data, to a hundredth of a pixel. */
export function pathData(points: readonly GraphPoint[]): string {
  const round = (n: number) => Math.round(n * 100) / 100;
  return points
    .map((point, i) => `${i === 0 ? 'M' : 'L'}${round(point.x)} ${round(point.y)}`)
    .join(' ');
}

/**
 * Pair the editor's geometry with the rail's, row by row, so any height in the
 * editor can be carried onto the rail.
 *
 * Each listed row claims the stretch of the editor it stands for:
 *
 * - a **setting** claims its own field row;
 * - a **folded section**, or one listing nothing, claims the whole section;
 * - an **open section** claims only its header — from its top down to its first
 *   listed setting — because the settings below speak for the rest.
 *
 * What falls between two claims (the gap between sections, the tail of a section
 * below its last listed setting, a setting the filter left out) is stretched
 * across the seam between their rows. Content above the first section and below
 * the last pins to the ends of the line.
 *
 * Anchors never decrease in either coordinate, whatever the measurements say,
 * so {@link toRail} is monotonic: a window that grows in the editor never
 * shrinks on the rail.
 */
export function railAnchors(
  sections: readonly OutlineSection[],
  isExpanded: (id: string) => boolean,
  spans: ReadonlyMap<string, OutlineSpan>,
  rows: ReadonlyMap<string, RailRow>,
): RailAnchor[] {
  const anchors: RailAnchor[] = [];
  const claim = (span: OutlineSpan | undefined, row: RailRow | undefined, end?: number) => {
    if (!span || !row) {
      return;
    }
    const last = anchors[anchors.length - 1];
    const fromTop = Math.max(span.top, last?.from ?? -Infinity);
    const toTop = Math.max(row.top, last?.to ?? -Infinity);
    anchors.push({ from: fromTop, to: toTop });
    anchors.push({
      from: Math.max(end ?? span.bottom, fromTop),
      to: Math.max(row.bottom, toTop),
    });
  };
  for (const section of sections) {
    const listed = isExpanded(section.id)
      ? section.entries.filter((entry) => rows.has(entry.id) && spans.has(entry.id))
      : [];
    const header = spans.get(section.id);
    if (listed.length === 0) {
      claim(header, rows.get(section.id));
      continue;
    }
    claim(header, rows.get(section.id), spans.get(listed[0].id)!.top);
    for (const entry of listed) {
      claim(spans.get(entry.id), rows.get(entry.id));
    }
  }
  return anchors;
}

/** Carry a height in the editor onto the rail through `anchors`. */
export function toRail(anchors: readonly RailAnchor[], y: number): number {
  if (anchors.length === 0) {
    return 0;
  }
  if (y <= anchors[0].from) {
    return anchors[0].to;
  }
  const last = anchors[anchors.length - 1];
  if (y >= last.from) {
    return last.to;
  }
  // Binary search for the last anchor at or above y; there are two per row, and
  // a print profile lists over two hundred rows once every section is open.
  let lo = 0;
  let hi = anchors.length - 1;
  while (hi - lo > 1) {
    const mid = (lo + hi) >> 1;
    if (anchors[mid].from <= y) {
      lo = mid;
    } else {
      hi = mid;
    }
  }
  const a = anchors[lo];
  const b = anchors[hi];
  const t = b.from === a.from ? 1 : (y - a.from) / (b.from - a.from);
  return a.to + (b.to - a.to) * t;
}
