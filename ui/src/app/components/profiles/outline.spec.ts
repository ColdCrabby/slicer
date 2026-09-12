import { beforeEach, describe, expect, it } from 'vitest';
import { filterOutline, idsInView, scanOutline, type OutlineSpan } from './outline';

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
