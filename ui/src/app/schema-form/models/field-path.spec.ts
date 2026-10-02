import { patchForPath, valueAtPath } from './field-path';

describe('field-path', () => {
  describe('valueAtPath', () => {
    it('reads a top-level key unchanged', () => {
      expect(valueAtPath({ layer_height: 0.2 }, 'layer_height')).toBe(0.2);
    });

    it('reads through a namespace', () => {
      const values = { plugins: { 'fuzzy-skin': { enabled: true } } };
      expect(valueAtPath(values, 'plugins.fuzzy-skin.enabled')).toBe(true);
    });

    it('returns undefined rather than throwing on a missing branch', () => {
      // A plugin the user has never touched has no namespace at all, and the
      // form must fall back to the schema default instead of crashing.
      expect(valueAtPath({}, 'plugins.fuzzy-skin.enabled')).toBeUndefined();
      expect(valueAtPath({ plugins: null }, 'plugins.a.b')).toBeUndefined();
      expect(valueAtPath({ plugins: 7 }, 'plugins.a')).toBeUndefined();
    });
  });

  describe('patchForPath', () => {
    it('produces the plain shallow patch for a top-level key', () => {
      expect(patchForPath({}, 'layer_height', 0.3)).toEqual({ layer_height: 0.3 });
    });

    it('rebuilds the whole root branch so a spread merge still works', () => {
      const values = { plugins: { 'fuzzy-skin': { enabled: true, amount: 0.3 } } };
      expect(patchForPath(values, 'plugins.fuzzy-skin.amount', 0.5)).toEqual({
        plugins: { 'fuzzy-skin': { enabled: true, amount: 0.5 } },
      });
    });

    it('keeps sibling namespaces', () => {
      // Whole-object patches are how settings are written, so dropping a
      // sibling here would silently wipe another plugin's settings.
      const values = { plugins: { a: { enabled: true }, b: { enabled: true } } };
      expect(patchForPath(values, 'plugins.a.enabled', false)).toEqual({
        plugins: { a: { enabled: false }, b: { enabled: true } },
      });
    });

    it('creates missing intermediate objects', () => {
      expect(patchForPath({}, 'plugins.demo.enabled', true)).toEqual({
        plugins: { demo: { enabled: true } },
      });
    });

    it('does not mutate the source', () => {
      const values = { plugins: { demo: { enabled: false } } };
      patchForPath(values, 'plugins.demo.enabled', true);
      expect(values.plugins.demo.enabled).toBe(false);
    });
  });
});
