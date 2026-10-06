import { describe, expect, it } from 'vitest';
import type { FieldDef } from '../schema-form/models/field-def';
import {
  browseScope,
  describeValue,
  narrowToScopes,
  searchOmniboxEntries,
  type OmniboxEntry,
} from './omnibox-entries';

const entry = (
  overrides: Partial<OmniboxEntry> & Pick<OmniboxEntry, 'id' | 'scopeId' | 'title'>,
): OmniboxEntry => ({
  kind: 'navigate',
  where: '',
  icon: 'search',
  ...overrides,
});

const SETTINGS = entry({
  id: 'settings:general',
  scopeId: 'settings',
  title: 'General',
  where: 'Settings',
  rank: 40,
});

const FILAMENT = entry({
  id: 'filaments:f1',
  scopeId: 'filaments',
  title: 'Prusament PETG',
  where: 'Filaments',
});

const PRINTER = entry({
  id: 'printers:p1',
  scopeId: 'printers',
  title: 'Voron 2.4',
  where: 'Printers',
});

const PROCESS = entry({
  id: 'processes:pr1',
  scopeId: 'processes',
  title: '0.2mm Standard',
  where: 'Processes',
});

const PARAM = entry({
  id: 'print:bed_temp',
  scopeId: 'print',
  kind: 'quickset',
  title: 'Bed Temperature',
  where: 'This plate · Plate 1',
  keywords: 'bed temp heat',
});

const ALL = [SETTINGS, FILAMENT, PRINTER, PROCESS, PARAM];

describe('narrowToScopes', () => {
  it('keeps everything when nothing is locked', () => {
    expect(narrowToScopes(ALL, [])).toHaveLength(ALL.length);
  });

  it('keeps only the locked scopes — THE acceptance test', () => {
    // A Settings lock means *exclusively* Settings: filament profiles,
    // printers and slicing presets are not ranked lower, they are gone.
    const narrowed = narrowToScopes(ALL, ['settings']);
    expect(narrowed).toEqual([SETTINGS]);
  });

  it('keeps the union of two locked scopes', () => {
    const narrowed = narrowToScopes(ALL, ['settings', 'print']);
    expect(narrowed).toEqual([SETTINGS, PARAM]);
  });
});

describe('searchOmniboxEntries', () => {
  it('finds entries by title', () => {
    expect(searchOmniboxEntries(ALL, 'prusament').map((e) => e.id)).toEqual([FILAMENT.id]);
  });

  it('finds entries by keyword', () => {
    expect(searchOmniboxEntries(ALL, 'heat').map((e) => e.id)).toEqual([PARAM.id]);
  });

  it('matches every word of a multi-word query', () => {
    expect(searchOmniboxEntries(ALL, 'bed temperature').map((e) => e.id)).toEqual([PARAM.id]);
    expect(searchOmniboxEntries(ALL, 'bed prusament')).toEqual([]);
  });

  it('matches word starts in the title, not fuzzy fragments', () => {
    expect(searchOmniboxEntries(ALL, 'bed te').map((e) => e.id)).toEqual([PARAM.id]);
    expect(searchOmniboxEntries(ALL, 'bed tp')).toEqual([]);
  });

  it('ranks a title match above a keyword match', () => {
    const heat = entry({
      id: 'print:heat',
      scopeId: 'print',
      title: 'Heated Bed',
      where: '',
    });
    const other = entry({
      id: 'print:other',
      scopeId: 'print',
      title: 'Unrelated',
      where: '',
      keywords: 'heated bed',
    });
    expect(searchOmniboxEntries([other, heat], 'heated bed')[0].id).toBe(heat.id);
  });

  it('floats a current-view result above an otherwise equal global one', () => {
    const here = entry({
      id: 'print:bed_temp',
      scopeId: 'print',
      title: 'Bed Temperature',
      where: 'This plate · Plate 1',
      currentView: true,
    });
    const there = entry({
      id: 'printers:bed_temp',
      scopeId: 'printers',
      title: 'Bed Temperature',
      where: 'Printers',
    });
    expect(searchOmniboxEntries([there, here], 'bed temp').map((e) => e.id)).toEqual([
      here.id,
      there.id,
    ]);
  });

  it('lets a clearly better global match outrank a weak current-view one', () => {
    const here = entry({
      id: 'print:faint',
      scopeId: 'print',
      title: 'Unrelated',
      where: 'This plate · Plate 1',
      keywords: 'bed temp',
      currentView: true,
    });
    const there = entry({
      id: 'printers:bed_temp',
      scopeId: 'printers',
      title: 'Bed Temperature',
      where: 'Printers',
    });
    expect(searchOmniboxEntries([here, there], 'bed temp')[0].id).toBe(there.id);
  });

  it('never answers an empty query', () => {
    expect(searchOmniboxEntries(ALL, '   ')).toEqual([]);
  });

  it('honours the lock while searching — THE acceptance test, end to end', () => {
    const results = searchOmniboxEntries(ALL, 'temperature', ['settings']);
    expect(results).toEqual([]);
    expect(searchOmniboxEntries(ALL, 'prusament', ['settings'])).toEqual([]);
    expect(searchOmniboxEntries(ALL, 'general', ['settings']).map((e) => e.id)).toEqual([
      SETTINGS.id,
    ]);
  });

  it('caps the result list', () => {
    const many = Array.from({ length: 50 }, (_, n) =>
      entry({ id: `print:f${n}`, scopeId: 'print', title: `Fill ${n}` }),
    );
    expect(searchOmniboxEntries(many, 'fill')).toHaveLength(30);
  });
});

describe('browseScope', () => {
  it("shows a locked scope's wares in rank order with an empty query", () => {
    const second = entry({
      id: 'settings:second',
      scopeId: 'settings',
      title: 'Second page',
      rank: 20,
    });
    expect(browseScope([SETTINGS, FILAMENT, second], ['settings']).map((e) => e.id)).toEqual([
      SETTINGS.id,
      second.id,
    ]);
  });

  it('breaks rank ties alphabetically', () => {
    const a = entry({ id: 's:a', scopeId: 'settings', title: 'Aaa', rank: 40 });
    const b = entry({ id: 's:b', scopeId: 'settings', title: 'Bbb', rank: 40 });
    expect(browseScope([b, a], ['settings']).map((e) => e.id)).toEqual([a.id, b.id]);
  });
});

describe('describeValue', () => {
  const field = (overrides: Partial<FieldDef>): FieldDef =>
    ({ key: 'k', type: 'double', ...overrides }) as unknown as FieldDef;

  it('reads booleans as On and Off', () => {
    expect(describeValue(undefined, true)).toBe('On');
    expect(describeValue(undefined, false)).toBe('Off');
  });

  it('shows a fraction as a percent', () => {
    expect(describeValue(field({ unit: 'fraction' }), 0.42)).toBe('42 %');
  });

  it('shows a percent and a ratio with their marks', () => {
    expect(describeValue(field({ unit: 'percent' }), 105)).toBe('105 %');
    expect(describeValue(field({ unit: 'ratio' }), 0.4)).toBe('0.4 ×');
  });

  it('labels an enum value', () => {
    const f = field({
      type: 'string',
      enumOptions: [{ value: 'miter', label: 'Miter' }],
    });
    expect(describeValue(f, 'miter')).toBe('Miter');
  });

  it('falls back to a dash for an absent value', () => {
    expect(describeValue(field({}), undefined)).toBe('—');
  });
});
