import { describe, expect, it } from 'vitest';
import { parseSchema } from './schema-parser';

/**
 * The parser is where engine prose becomes UI text. Both normalisations happen
 * here so that a `FieldDef` is plain, showable text and no widget owns a copy
 * of the rule — widgets used to strip Markdown themselves, which is how the
 * same description read one way in the profile editor and another in the
 * tooltip two clicks away.
 */
describe('parseSchema text', () => {
  function parse(props: Record<string, unknown>, defs: Record<string, unknown> = {}) {
    return parseSchema({ properties: props, $defs: defs }).fields;
  }

  it('strips the Markdown the engine emits from a field description', () => {
    const [field] = parse({
      layer_height: {
        type: 'number',
        'x-group': 'Layer',
        description: 'Layer height in mm.\n\n**Typical:** 0.05–0.35 mm, set by `layer_height`.',
      },
    });
    expect(field.description).toBe(
      'Layer height in mm.\n\nTypical: 0.05–0.35 mm, set by layer_height.',
    );
  });

  it('keeps paragraph breaks, which the editor renders as blank lines', () => {
    const [field] = parse({
      a: { type: 'number', 'x-group': 'G', description: 'One.\n\nTwo.' },
    });
    expect(field.description).toContain('\n\n');
  });

  it('takes an option summary from the doc comment, ending at the blank line', () => {
    const [field] = parse(
      { wall_generator: { $ref: '#/$defs/WallGenerator', 'x-group': 'Walls' } },
      {
        WallGenerator: {
          oneOf: [
            {
              const: 'classic',
              // Hard-wrapped the way rustdoc writes it, with the detail below.
              description:
                'Every wall the\nsame width.\n\nDeterministic, and the rest of the essay.',
            },
          ],
        },
      },
    );
    expect(field.enumOptions?.[0].description).toBe('Every wall the same width.');
  });

  it('leaves a single-paragraph summary alone', () => {
    const [field] = parse(
      { a: { $ref: '#/$defs/E', 'x-group': 'G' } },
      { E: { oneOf: [{ const: 'x', description: 'Just the one line.' }] } },
    );
    expect(field.enumOptions?.[0].description).toBe('Just the one line.');
  });

  it('reports no summary rather than an empty one', () => {
    const [field] = parse(
      { a: { $ref: '#/$defs/E', 'x-group': 'G' } },
      { E: { oneOf: [{ const: 'x' }] } },
    );
    expect(field.enumOptions?.[0].description).toBeUndefined();
  });
});
