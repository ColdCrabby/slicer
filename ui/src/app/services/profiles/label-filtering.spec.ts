import { describe, expect, it } from 'vitest';
import { matchesAnyLabel, toggledFilter, toggledLabelIds } from './label-filtering';

const pla = { label_ids: ['pla'] };
const petg = { label_ids: ['petg'] };
const both = { label_ids: ['pla', 'petg'] };
const none = {};

describe('matchesAnyLabel', () => {
  it('matches everything when nothing is selected', () => {
    expect(matchesAnyLabel(none, [])).toBe(true);
    expect(matchesAnyLabel(pla, [])).toBe(true);
  });

  it('widens the list as labels are added, rather than narrowing it', () => {
    // OR, not AND: selecting PLA and then PETG asks to see both. Requiring
    // every label emptied the list on the second click unless some profile
    // happened to carry the pair.
    expect(matchesAnyLabel(pla, ['pla', 'petg'])).toBe(true);
    expect(matchesAnyLabel(petg, ['pla', 'petg'])).toBe(true);
    expect(matchesAnyLabel(both, ['pla', 'petg'])).toBe(true);
  });

  it('excludes a profile carrying none of the selected labels', () => {
    expect(matchesAnyLabel(none, ['pla'])).toBe(false);
    expect(matchesAnyLabel(petg, ['pla'])).toBe(false);
  });
});

describe('toggling', () => {
  it('adds a label that is absent and removes one that is present', () => {
    expect(toggledLabelIds(['pla'], 'petg')).toEqual(['pla', 'petg']);
    expect(toggledLabelIds(['pla', 'petg'], 'pla')).toEqual(['petg']);
    expect(toggledLabelIds(undefined, 'pla')).toEqual(['pla']);
  });

  it('treats the filter set the same way', () => {
    expect(toggledFilter([], 'pla')).toEqual(['pla']);
    expect(toggledFilter(['pla'], 'pla')).toEqual([]);
  });
});
