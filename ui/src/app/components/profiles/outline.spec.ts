import { beforeEach, describe, expect, it } from 'vitest';
import {
  EDITOR_MIN_WIDTH,
  GRAPH_LANES,
  RAIL_WIDTH,
  filterOutline,
  graphLine,
  hasRoomForRail,
  idsInView,
  pathData,
  railAnchors,
  railNeedsFold,
  scanOutline,
  sliceLine,
  toRail,
  type OutlineSection,
  type OutlineSpan,
  type RailRow,
} from './outline';

function editor(html: string): HTMLElement {
  const root = document.createElement('div');
  root.innerHTML = html;
  return root;
}

const EDITOR = `
  <div class="profile-editor__group">
    <label class="profile-editor__group-title">Identity</label>
    <nexus-field-row><span class="field-row__title">Name</span></nexus-field-row>
  </div>
  <div class="profile-editor__group">
    <label class="profile-editor__group-title">Walls</label>
    <nexus-field-shell><span class="field-shell__title">Wall Count</span></nexus-field-shell>
    <nexus-field-shell><span class="field-shell__title">Seam Position</span></nexus-field-shell>
  </div>
`;

describe('scanOutline', () => {
  let root: HTMLElement;

  beforeEach(() => {
    root = editor(EDITOR);
  });

  it('reads a section per group and a row per field', () => {
    const sections = scanOutline(root);
    expect(sections.map((s) => s.title)).toEqual(['Identity', 'Walls']);
    expect(sections[1].entries.map((e) => e.title)).toEqual(['Wall Count', 'Seam Position']);
  });

  it('points each row at the element a jump should scroll to', () => {
    const [identity] = scanOutline(root);
    expect(identity.entries[0].el.tagName.toLowerCase()).toBe('nexus-field-row');
  });

  it('skips a group that renders no title', () => {
    root = editor('<div class="profile-editor__group"><span>loose</span></div>' + EDITOR);
    expect(scanOutline(root)).toHaveLength(2);
  });

  // The corrections section: a list of materials, each holding settings that
  // must not be listed under it. Two materials correcting the same setting
  // would otherwise put the same words in the rail twice, and bury the three
  // names the section is actually read by.
  it('lists an opted-in heading as a row and skips a subtree marked to skip', () => {
    root = editor(`
      <div class="profile-editor__group">
        <label class="profile-editor__group-title">Material corrections</label>
        <nexus-field-row><span class="field-row__title">Correct a material</span></nexus-field-row>
        <section>
          <h4 class="outline-entry-title">PLA</h4>
          <div data-outline-skip>
            <nexus-field-shell><span class="field-shell__title">Max Volumetric Speed</span></nexus-field-shell>
          </div>
        </section>
        <section>
          <h4 class="outline-entry-title">ABS</h4>
          <div data-outline-skip>
            <nexus-field-shell><span class="field-shell__title">Max Volumetric Speed</span></nexus-field-shell>
          </div>
        </section>
      </div>
    `);

    const [corrections] = scanOutline(root);
    expect(corrections.entries.map((e) => e.title)).toEqual(['Correct a material', 'PLA', 'ABS']);
  });

  it('keeps repeated titles distinguishable', () => {
    root = editor(EDITOR + EDITOR);
    const ids = scanOutline(root).map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});

describe('filterOutline', () => {
  const sections = scanOutline(editor(EDITOR));

  it('returns everything for a blank query', () => {
    expect(filterOutline(sections, '  ')).toHaveLength(2);
  });

  it('keeps the section a matching row lives in, and drops the rest', () => {
    const result = filterOutline(sections, 'seam');
    expect(result.map((s) => s.title)).toEqual(['Walls']);
    expect(result[0].entries.map((e) => e.title)).toEqual(['Seam Position']);
  });

  it('gives a whole section when the section name itself matches', () => {
    expect(filterOutline(sections, 'wall')[0].entries).toHaveLength(2);
  });

  it('matches regardless of case', () => {
    expect(filterOutline(sections, 'NAME')[0].title).toBe('Identity');
  });
});

describe('idsInView', () => {
  const spans = new Map<string, OutlineSpan>([
    ['a', { top: 0, bottom: 100 }],
    ['b', { top: 100, bottom: 200 }],
    ['c', { top: 200, bottom: 300 }],
  ]);

  it('returns every row wholly inside the window, not just the first', () => {
    expect([...idsInView(spans, 0, 300)]).toEqual(['a', 'b', 'c']);
  });

  it('leaves out a row the window only partly covers', () => {
    expect([...idsInView(spans, 50, 250)]).toEqual(['b']);
  });

  it('counts a row that exactly fills the window', () => {
    expect([...idsInView(spans, 100, 200)]).toEqual(['b']);
  });

  it('is empty past the end of the content', () => {
    expect(idsInView(spans, 400, 500).size).toBe(0);
  });
});

describe('hasRoomForRail', () => {
  const GAP = 16;
  const LIST = 260;
  /** What the body must measure for the editor to land exactly on its minimum. */
  const EXACT = EDITOR_MIN_WIDTH + LIST + GAP * 2 + RAIL_WIDTH;

  it('gives the rail a column only once the editor has its minimum width', () => {
    expect(hasRoomForRail(EXACT, LIST, GAP)).toBe(true);
    expect(hasRoomForRail(EXACT - 1, LIST, GAP)).toBe(false);
  });

  // A form narrower than this starts wrapping its rows onto two lines, and a
  // map of the page is not worth doubling the page's height.
  it('counts the editor as a two-column form plus both gutters', () => {
    expect(EDITOR_MIN_WIDTH).toBe(448 + 16 * 2);
  });

  it('gives the column back as the list column is dragged wider', () => {
    expect(hasRoomForRail(EXACT, LIST, GAP)).toBe(true);
    expect(hasRoomForRail(EXACT, LIST + 40, GAP)).toBe(false);
  });

  // The measurement has to be taken on something the rail does not resize, or
  // showing it would remove the room that showed it. This is the property that
  // makes the answer stable rather than oscillating: the same body width gives
  // the same answer whether the rail is currently up or not.
  it('is a pure function of the space, not of the rail being shown', () => {
    expect(hasRoomForRail(EXACT, LIST, GAP)).toBe(hasRoomForRail(EXACT, LIST, GAP));
    expect(hasRoomForRail(1600, LIST, GAP)).toBe(true);
    expect(hasRoomForRail(700, LIST, GAP)).toBe(false);
  });

  // The whole point of the budget: an M4 iPad held sideways shows the outline.
  // The window minus the 60 px app rail, the folded 56 px section list and the
  // page's 16 px gutters is what the manage body gets.
  it('fits on an 11-inch and a 13-inch M4 iPad in landscape', () => {
    const body = (window: number, sectionList: number) => window - 60 - sectionList - 16 * 2;
    expect(hasRoomForRail(body(1180, 56), LIST, GAP)).toBe(true); // iPad Air 11"
    expect(hasRoomForRail(body(1210, 56), LIST, GAP)).toBe(true); // iPad Pro 11"
    expect(hasRoomForRail(body(1376, 200), LIST, GAP)).toBe(true); // iPad Pro 13", list open
  });
});

describe('railNeedsFold', () => {
  const GAP = 16;
  const LIST = 260;
  const EXACT = EDITOR_MIN_WIDTH + LIST + GAP * 2 + RAIL_WIDTH;

  it('asks for the fold only when folding is what makes the room', () => {
    expect(railNeedsFold(EXACT - 144, EXACT, LIST, GAP)).toBe(true);
  });

  it('leaves the section list alone when there is room with it open', () => {
    expect(railNeedsFold(EXACT, EXACT + 144, LIST, GAP)).toBe(false);
  });

  // Folding there would cost the labels and still not show the outline.
  it('leaves it alone when there is no room even folded', () => {
    expect(railNeedsFold(EXACT - 400, EXACT - 256, LIST, GAP)).toBe(false);
  });
});

/** Rows of a fixed height, stacked with a gap, the way the rail lays them out. */
function stack(depths: number[], height = 24, gap = 2): RailRow[] {
  let y = 0;
  return depths.map((depth, i) => {
    const row = { id: `r${i}`, depth, top: y, bottom: y + height };
    y += height + gap;
    return row;
  });
}

describe('graphLine', () => {
  const [section, setting] = GRAPH_LANES;

  it('runs straight down a folded outline in two points', () => {
    const line = graphLine(stack([0, 0, 0]));
    expect(line).toEqual([
      { x: section, y: 0 },
      { x: section, y: 76 },
    ]);
  });

  it('swings in under an open section and back out before the next one', () => {
    const line = graphLine(stack([0, 1, 1, 0]));
    const xs = line.map((p) => p.x);
    expect(xs[0]).toBe(section);
    expect(Math.max(...xs)).toBe(setting);
    expect(xs[xs.length - 1]).toBe(section);
  });

  // The bend is centred on the seam between the rows and kept inside them, so
  // the line reaches a row's lane before the row's middle.
  it('bends across the seam, never past the middle of either row', () => {
    const rows = stack([0, 1]);
    const line = graphLine(rows);
    const bend = line.filter((p) => p.x !== section && p.x !== setting);
    const seam = (rows[0].bottom + rows[1].top) / 2;
    for (const p of bend) {
      expect(p.y).toBeGreaterThan((rows[0].top + rows[0].bottom) / 2);
      expect(p.y).toBeLessThan((rows[1].top + rows[1].bottom) / 2);
    }
    expect(bend.some((p) => p.y < seam) && bend.some((p) => p.y > seam)).toBe(true);
  });

  it('never goes back up, so a band of height cuts it once', () => {
    const line = graphLine(stack([0, 1, 1, 0, 1, 0, 0, 1]));
    for (let i = 1; i < line.length; i++) {
      expect(line[i].y).toBeGreaterThanOrEqual(line[i - 1].y);
    }
  });

  it('is empty for an empty outline', () => {
    expect(graphLine([])).toEqual([]);
  });
});

describe('sliceLine', () => {
  const line = graphLine(stack([0, 1, 1, 0]));

  it('starts and stops exactly at the band it was asked for', () => {
    const slice = sliceLine(line, 10, 60);
    expect(slice[0].y).toBe(10);
    expect(slice[slice.length - 1].y).toBe(60);
  });

  it('keeps the bends that fall inside the band', () => {
    const slice = sliceLine(line, 0, 104);
    expect(new Set(slice.map((p) => p.x)).size).toBeGreaterThan(2);
  });

  it('interpolates onto the line mid-bend rather than snapping to a lane', () => {
    const rows = stack([0, 1]);
    const seam = (rows[0].bottom + rows[1].top) / 2;
    const [start] = sliceLine(graphLine(rows), seam, rows[1].bottom);
    expect(start.x).toBeGreaterThan(GRAPH_LANES[0]);
    expect(start.x).toBeLessThan(GRAPH_LANES[1]);
  });

  it('clips a band that runs past either end of the line', () => {
    const slice = sliceLine(line, -50, 500);
    expect(slice[0].y).toBe(0);
    expect(slice[slice.length - 1].y).toBe(line[line.length - 1].y);
  });

  it('is empty for a band that misses the line or has no height', () => {
    expect(sliceLine(line, 200, 300)).toEqual([]);
    expect(sliceLine(line, 30, 30)).toEqual([]);
  });
});

describe('pathData', () => {
  it('writes a move then lines, rounded to a hundredth', () => {
    expect(
      pathData([
        { x: 7, y: 0 },
        { x: 7.123, y: 10.5 },
      ]),
    ).toBe('M7 0 L7.12 10.5');
  });
});

describe('railAnchors and toRail', () => {
  const el = document.createElement('div');
  const section = (id: string, entries: string[]): OutlineSection => ({
    id,
    title: id,
    el,
    entries: entries.map((e) => ({ id: e, title: e, el })),
  });

  // Two sections in the editor: A is 0–400 with its header down to 100 and two
  // settings at 100–250 and 250–400; B is 420–1000 with nothing listed.
  const sections = [section('A', ['a1', 'a2']), section('B', [])];
  const spans = new Map<string, OutlineSpan>([
    ['A', { top: 0, bottom: 400 }],
    ['a1', { top: 100, bottom: 250 }],
    ['a2', { top: 250, bottom: 400 }],
    ['B', { top: 420, bottom: 1000 }],
  ]);

  it('maps an open section header, each setting, and a folded section onto their rows', () => {
    const rows = new Map<string, RailRow>(
      stack([0, 1, 1, 0], 20, 0).map((row, i) => [['A', 'a1', 'a2', 'B'][i], row]),
    );
    const anchors = railAnchors(sections, () => true, spans, rows);
    expect(toRail(anchors, 0)).toBe(0);
    expect(toRail(anchors, 100)).toBe(20); // A's header ends where a1 starts
    expect(toRail(anchors, 175)).toBe(30); // halfway down a1
    expect(toRail(anchors, 1000)).toBe(80); // the bottom of B
  });

  it('maps a whole folded section onto its one row', () => {
    const rows = new Map<string, RailRow>([
      ['A', { id: 'A', depth: 0, top: 0, bottom: 20 }],
      ['B', { id: 'B', depth: 0, top: 20, bottom: 40 }],
    ]);
    const anchors = railAnchors(sections, () => false, spans, rows);
    expect(toRail(anchors, 200)).toBe(10);
    expect(toRail(anchors, 710)).toBe(30);
  });

  // The window is the point: it slides pixel by pixel, not row by row.
  it('moves continuously as the editor scrolls', () => {
    const rows = new Map<string, RailRow>(
      stack([0, 1, 1, 0], 20, 0).map((row, i) => [['A', 'a1', 'a2', 'B'][i], row]),
    );
    const anchors = railAnchors(sections, () => true, spans, rows);
    const a = toRail(anchors, 150);
    const b = toRail(anchors, 151);
    expect(b).toBeGreaterThan(a);
    expect(b - a).toBeLessThan(1);
  });

  it('pins content above the first section and below the last to the ends', () => {
    const rows = new Map<string, RailRow>([
      ['A', { id: 'A', depth: 0, top: 4, bottom: 24 }],
      ['B', { id: 'B', depth: 0, top: 26, bottom: 46 }],
    ]);
    const anchors = railAnchors(sections, () => false, spans, rows);
    expect(toRail(anchors, -100)).toBe(4);
    expect(toRail(anchors, 5000)).toBe(46);
  });

  it('never runs backwards, even over measurements that overlap', () => {
    const rows = new Map<string, RailRow>(
      stack([0, 1, 1, 0], 20, 0).map((row, i) => [['A', 'a1', 'a2', 'B'][i], row]),
    );
    const overlapping = new Map(spans).set('a2', { top: 200, bottom: 380 });
    const anchors = railAnchors(sections, () => true, overlapping, rows);
    for (let i = 1; i < anchors.length; i++) {
      expect(anchors[i].from).toBeGreaterThanOrEqual(anchors[i - 1].from);
      expect(anchors[i].to).toBeGreaterThanOrEqual(anchors[i - 1].to);
    }
  });

  it('is zero for an outline with nothing drawn', () => {
    expect(toRail([], 300)).toBe(0);
  });
});
