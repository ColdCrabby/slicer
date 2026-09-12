import { describe, expect, it } from 'vitest';
import processSchema from '../../../schemas/slicer-engine-process-profile-v1.json';
import { SEGMENTED_MAX_OPTIONS, controlFor } from './field-control';
import type { FieldDef } from './field-def';
import { optionSummary } from './field-labels';
import { parseSchema } from './schema-parser';

function field(extra: Partial<FieldDef> = {}): FieldDef {
  return { key: 'x', type: 'number', required: false, ...extra };
}

function options(n: number) {
  return Array.from({ length: n }, (_, i) => ({ value: `v${i}`, label: `V${i}` }));
}

describe('controlFor', () => {
  it('gives a boolean a switch, never a two-option control', () => {
    // On/off is not a choice between two things, and rendering it as one makes
    // every checkbox in the panel look like a mode selector.
    expect(controlFor(field({ type: 'boolean' }))).toBe('switch');
  });

  it('splits enums between segmented and select on option count alone', () => {
    expect(controlFor(field({ enumOptions: options(2) }))).toBe('segmented');
    expect(controlFor(field({ enumOptions: options(SEGMENTED_MAX_OPTIONS) }))).toBe('segmented');
    expect(controlFor(field({ enumOptions: options(SEGMENTED_MAX_OPTIONS + 1) }))).toBe('select');
  });

  it('only gives option cards to a field that asks for them', () => {
    // Cards are the tallest control the form has. Handing them to every short
    // enum is how a thumbnail's Light/Dark/Transparent came to occupy as much
    // of the sidebar as the wall generator.
    expect(controlFor(field({ enumOptions: options(2) }))).toBe('segmented');
    expect(controlFor(field({ enumOptions: options(2), widget: 'cards' }))).toBe('cards');
  });

  it('never sends a string to a number input', () => {
    // The old default fell through to a number spinner, which is how a
    // filament's name and colour became steppers.
    expect(controlFor(field({ type: 'string' }))).toBe('text');
    expect(controlFor(field({ key: 'filament_color', type: 'string' }))).toBe('color');
    expect(controlFor(field({ type: 'string', widget: 'gcode' }))).toBe('gcode');
  });

  it('lets a key override outrank the schema hint', () => {
    expect(controlFor(field({ key: 'infill_density', type: 'number', widget: 'cards' }))).toBe(
      'slider',
    );
  });
});

describe('optionSummary', () => {
  it('keeps the opening sentence and drops the essay', () => {
    expect(optionSummary('Grid infill. Strong in every axis. Slower than lines.')).toBe(
      'Grid infill.',
    );
  });

  it('strips the Markdown the engine emits', () => {
    expect(optionSummary('Uses **medial axis** and `wall_count`.')).toBe(
      'Uses medial axis and wall_count.',
    );
  });

  it('survives a description that is missing or has no full stop', () => {
    expect(optionSummary(undefined)).toBe('');
    expect(optionSummary('Organic branches')).toBe('Organic branches');
  });

  it('does not end the sentence at an abbreviation', () => {
    // The RepRap flavour used to read "…a few RRF-specific commands (e.g." —
    // the word after the stop is capitalised, so nothing but the abbreviation
    // itself says it is mid-sentence.
    expect(optionSummary('A baseline plus a few extras (e.g. M226 to pause). And more.')).toBe(
      'A baseline plus a few extras (e.g. M226 to pause).',
    );
  });

  it('does not end the sentence inside a number', () => {
    expect(optionSummary('Scales the estimate by 1.25 for this machine. Details follow.')).toBe(
      'Scales the estimate by 1.25 for this machine.',
    );
  });
});

describe('the engine schema', () => {
  const schema = processSchema as unknown as { $defs: Record<string, Record<string, unknown>> };
  const fields = parseSchema({ ...schema.$defs['SlicingParams'], $defs: schema.$defs }).fields;

  /**
   * Cards are opt-in, and staying scarce is the point: a panel where every
   * branch is a stack of explanations is no calmer than one with no
   * explanations at all. This is a budget, not a law — raise it deliberately.
   */
  it('keeps option cards to the handful of decisions that earn them', () => {
    const cards = fields.filter((f) => controlFor(f) === 'cards').map((f) => f.key);
    expect(cards.length, `fields using option cards: ${cards.join(', ')}`).toBeLessThanOrEqual(6);
  });

  it('renders every parameter with a control from the shared set', () => {
    // `array` is the one kind no surface renders; everything else must land on
    // a real control rather than a fallback.
    const kinds = new Set(fields.map((f) => controlFor(f)));
    expect(kinds.has('array')).toBe(true);
    expect([...kinds].every((k) => k !== undefined)).toBe(true);
  });
});
