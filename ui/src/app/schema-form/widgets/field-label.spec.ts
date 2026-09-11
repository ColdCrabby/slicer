import { describe, expect, it } from 'vitest';

/**
 * The settings panel italicises a setting the user changed away from their
 * profile, and it finds the text to italicise by one hook: `.field-label` on
 * whatever element shows the field's name.
 *
 * Widgets legitimately disagree about what that element is — a `<label>` where
 * there is one control to point at, a `<span>` for a switch row or a radio
 * group's legend — so nothing can find it by tag. A widget that forgets the
 * class still renders perfectly and just stops being markable, which is exactly
 * the kind of thing nobody notices until a setting quietly looks inherited when
 * it is not. Hence a test rather than a convention.
 */
describe('widget labels', () => {
  const sources = (
    import.meta as unknown as {
      glob: (
        patterns: string[],
        options: { query: string; import: string; eager: boolean },
      ) => Record<string, string>;
    }
  ).glob(['../widgets/**/*.ts', '../custom-widgets/**/*.ts'], {
    query: '?raw',
    import: 'default',
    eager: true,
  });

  const widgets = Object.entries(sources).filter(
    ([path, source]) => !path.endsWith('.spec.ts') && source.includes('field().title'),
  );

  it('finds the widgets to check', () => {
    expect(widgets.length).toBeGreaterThanOrEqual(8);
  });

  it.each(widgets.map(([path]) => path))('%s marks its label with .field-label', (path) => {
    const source = sources[path];
    expect(
      source.includes('field-label'),
      `${path} renders the field's name without the hook`,
    ).toBe(true);
  });
});
