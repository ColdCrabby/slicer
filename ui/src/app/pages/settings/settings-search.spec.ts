import { describe, expect, it } from 'vitest';
import type { FieldDef, SchemaGroup } from '../../schema-form/models/field-def';
import { PREFS, prefById } from './prefs/pref-registry';
import { SETTINGS_SECTIONS } from './settings-sections';
import {
  preferenceEntries,
  profileEntries,
  searchSettings,
  sectionEntries,
  settingEntries,
  type SearchEntry,
} from './settings-search';

const field = (key: string, title: string, description = ''): FieldDef =>
  ({ key, title, description }) as unknown as FieldDef;

const groups = (
  printer: SchemaGroup[],
  filament: SchemaGroup[] = [],
  process: SchemaGroup[] = [],
) => ({ printer, filament, process });

const PARAMS = settingEntries(
  groups(
    [
      {
        name: 'Retraction',
        fields: [
          field(
            'retract_mm',
            'Retraction Distance',
            'Filament pulled back on travel. Other slicers call this the retraction length.',
          ),
        ],
      } as unknown as SchemaGroup,
    ],
    [
      {
        name: 'Temperature',
        fields: [
          field('bed_temp', 'Bed Temperature'),
          field(
            'nozzle_temp',
            'Nozzle Temperature',
            'Hotter flows faster and strings more after a retraction.',
          ),
        ],
      } as unknown as SchemaGroup,
    ],
  ),
);

const INDEX: SearchEntry[] = [
  ...sectionEntries(),
  ...preferenceEntries(),
  ...profileEntries({
    printer: [{ id: 'p1', name: 'Voron 2.4' }],
    filament: [{ id: 'f1', name: 'Prusament PETG' }],
    process: [],
  }),
  ...PARAMS,
];

const titles = (query: string) => searchSettings(INDEX, query).map((hit) => hit.title);

describe('searchSettings', () => {
  it('finds a page by its name', () => {
    expect(titles('appear')[0]).toBe('Appearance');
  });

  it('finds a preference by a word it is known by elsewhere', () => {
    expect(titles('dark mode')).toContain('Theme');
    expect(titles('hotkeys')).toContain('Keyboard shortcuts');
  });

  it('finds one of your own profiles by name', () => {
    const [hit] = searchSettings(INDEX, 'voron');
    expect(hit.title).toBe('Voron 2.4');
    expect(hit.queryParams).toEqual({ id: 'p1' });
  });

  it('needs every word, so a second word narrows rather than widens', () => {
    expect(titles('bed temp')).toEqual(['Bed Temperature']);
  });

  it('matches the name other slicers use, through the description', () => {
    expect(titles('retraction length')).toContain('Retraction Distance');
  });

  it('puts a title hit above one that only its description mentions', () => {
    const hits = titles('retraction');
    expect(hits).toContain('Nozzle Temperature');
    expect(hits.indexOf('Retraction Distance')).toBeLessThan(hits.indexOf('Nozzle Temperature'));
  });

  it('ranks a page above a parameter that shares the word', () => {
    const hits = titles('printer');
    expect(hits[0]).toBe('Printers');
  });

  it('returns nothing for a blank query', () => {
    expect(searchSettings(INDEX, '   ')).toEqual([]);
  });

  it('caps the list', () => {
    expect(searchSettings(INDEX, 'e', 3)).toHaveLength(3);
  });
});

describe('where results lead', () => {
  it('sends a preference to its page and its row', () => {
    const shadows = preferenceEntries().find((entry) => entry.title === 'Shadows')!;
    expect(shadows.path).toBe('/settings/3d-view');
    expect(shadows.target).toBe('#pref-shadows');
    expect(shadows.where).toBe('3D View · Look');
  });

  it('sends a parameter to its editor with the key to land on', () => {
    const bed = PARAMS.find((entry) => entry.title === 'Bed Temperature')!;
    expect(bed.path).toBe('/settings/filaments');
    expect(bed.queryParams).toEqual({ focus: 'bed_temp' });
  });

  it('lists the printer editor’s hand-built sections by the words people use', () => {
    expect(titles('api key')).toContain('Connection');
    expect(titles('bed size')).toContain('Build volume');
  });
});

describe('the preference registry', () => {
  it('gives every preference a unique id', () => {
    const ids = PREFS.map((pref) => pref.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it('puts every preference on a page the sidebar lists', () => {
    const paths = new Set(SETTINGS_SECTIONS.map((section) => section.path));
    for (const pref of PREFS) {
      expect(paths.has(pref.page), pref.id).toBe(true);
    }
  });

  // The line under a title is the glance; a second sentence belongs in the ⓘ.
  it('keeps every line under a title to one sentence', () => {
    for (const pref of PREFS) {
      const sentences = (pref.hint ?? '').split(/[.!?](\s|$)/).filter((s) => s.trim());
      expect(sentences.length, pref.id).toBeLessThanOrEqual(1);
    }
  });

  it('refuses an unknown id rather than rendering a blank row', () => {
    expect(() => prefById('no-such-preference')).toThrow();
  });
});
