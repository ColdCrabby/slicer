import { describe, expect, it } from 'vitest';
import { omniboxScopeDefs } from './scope-registry';
import {
  lockSuggestedScope,
  matchScopePrefix,
  parseScopeQuery,
  popLockedScope,
  suggestedScope,
  type ScopeDef,
} from './scope-tokens';

const scope = (id: string, tokens: string[]): ScopeDef => ({
  id,
  label: id,
  icon: 'search',
  tokens,
  hint: '',
});

const DEFS = [
  scope('settings', ['settings', 'preferences', 'prefs', 'app']),
  scope('filaments', ['filament', 'filaments', 'material']),
  scope('models', ['model', 'models', 'files']),
];

describe('matchScopePrefix', () => {
  it('matches an exact token outright', () => {
    expect(matchScopePrefix('settings', DEFS)?.id).toBe('settings');
  });

  it('matches by prefix', () => {
    expect(matchScopePrefix('sett', DEFS)?.id).toBe('settings');
    expect(matchScopePrefix('fil', DEFS)?.id).toBe('filaments');
  });

  it('is case-insensitive', () => {
    expect(matchScopePrefix('SETT', DEFS)?.id).toBe('settings');
  });

  it('gives an exact token of any scope priority over a prefix of an earlier one', () => {
    // `files` is a full token of `models`; it must win even though
    // `filaments` is listed first and `fi` prefixes it.
    expect(matchScopePrefix('files', DEFS)?.id).toBe('models');
  });

  it('breaks prefix ties by registry order', () => {
    // `material` (filaments, listed second) and `model` (models, third) both
    // extend `m`; the earlier scope wins.
    expect(matchScopePrefix('m', DEFS)?.id).toBe('filaments');
    expect(matchScopePrefix('mo', DEFS)?.id).toBe('models');
  });

  it('answers null for a word no scope claims', () => {
    expect(matchScopePrefix('printerz', DEFS)).toBeNull();
    expect(matchScopePrefix('', DEFS)).toBeNull();
  });
});

describe('parseScopeQuery', () => {
  it('parses nothing from an empty query', () => {
    expect(parseScopeQuery('', DEFS)).toEqual({ scope: null, rest: '', partial: false });
  });

  it('keeps a query with no leading scope token whole', () => {
    const parsed = parseScopeQuery('bed temperature', DEFS);
    expect(parsed.scope).toBeNull();
    expect(parsed.rest).toBe('bed temperature');
  });

  it('splits a scope token from the rest', () => {
    const parsed = parseScopeQuery('settings infill', DEFS);
    expect(parsed.scope?.id).toBe('settings');
    expect(parsed.rest).toBe('infill');
    expect(parsed.partial).toBe(false);
  });

  it('reports a prefix as partial', () => {
    const parsed = parseScopeQuery('sett infill', DEFS);
    expect(parsed.scope?.id).toBe('settings');
    expect(parsed.rest).toBe('infill');
    expect(parsed.partial).toBe(true);
  });

  it('only reads the first word as a scope', () => {
    const parsed = parseScopeQuery('bed settings', DEFS);
    expect(parsed.scope).toBeNull();
    expect(parsed.rest).toBe('bed settings');
  });

  it('keeps inner spacing of the rest', () => {
    expect(parseScopeQuery('settings  bed  temp', DEFS).rest).toBe('bed  temp');
  });
});

describe('locked scopes', () => {
  it('locks the suggested scope, once', () => {
    const parsed = parseScopeQuery('settings infill', DEFS);
    expect(lockSuggestedScope(parsed, [])).toEqual(['settings']);
    expect(lockSuggestedScope(parsed, ['settings'])).toEqual(['settings']);
  });

  it('locks nothing when the query names no scope', () => {
    expect(lockSuggestedScope(parseScopeQuery('infill', DEFS), [])).toEqual([]);
  });

  it('locks a union', () => {
    const parsed = parseScopeQuery('models cube', DEFS);
    expect(lockSuggestedScope(parsed, ['settings'])).toEqual(['settings', 'models']);
  });

  it('pops the last-locked scope', () => {
    expect(popLockedScope(['settings', 'models'])).toEqual(['settings']);
    expect(popLockedScope([])).toEqual([]);
  });

  it('suppresses the suggestion for an already-locked scope', () => {
    const parsed = parseScopeQuery('settings infill', DEFS);
    expect(suggestedScope(parsed, ['settings'])).toBeNull();
    expect(suggestedScope(parsed, [])?.id).toBe('settings');
  });
});

describe('the shipped scope registry', () => {
  it('resolves the documented prefixes', () => {
    const defs = omniboxScopeDefs();
    expect(matchScopePrefix('sett', defs)?.id).toBe('settings');
    expect(matchScopePrefix('s', defs)?.id).toBe('settings');
    expect(matchScopePrefix('p', defs)?.id).toBe('print');
    expect(matchScopePrefix('pr', defs)?.id).toBe('print');
    expect(matchScopePrefix('printer', defs)?.id).toBe('printers');
    expect(matchScopePrefix('f', defs)?.id).toBe('filaments');
    expect(matchScopePrefix('file', defs)?.id).toBe('models');
    expect(matchScopePrefix('prefs', defs)?.id).toBe('settings');
  });

  it('has unique ids and at least one token per scope', () => {
    const defs = omniboxScopeDefs();
    const ids = defs.map((def) => def.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const def of defs) {
      expect(def.tokens.length).toBeGreaterThan(0);
    }
  });
});
