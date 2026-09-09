import { parseSchema } from './schema-parser';
import { isFieldRelevant } from './relevance';

/**
 * The schema shape the engine emits for a build that ships one plugin: a
 * `plugins` bag whose properties are one namespace per plugin id, each
 * carrying the engine-supplied `enabled` toggle plus the plugin's own fields.
 */
const WITH_PLUGIN = {
  type: 'object',
  properties: {
    layer_height: { type: 'number', 'x-group': 'Layer' },
    plugins: {
      type: 'object',
      properties: {
        demo: {
          type: 'object',
          properties: {
            enabled: { type: 'boolean', title: 'Demo', 'x-group': 'Experiments' },
            amount: {
              type: 'number',
              title: 'Amount',
              'x-group': 'Experiments',
              'x-relevant-when': { field: 'plugins.demo.enabled', equals: true },
            },
          },
        },
      },
    },
  },
};

describe('parseSchema', () => {
  it('keys a namespaced field by its dotted path', () => {
    const { fields } = parseSchema(WITH_PLUGIN);
    expect(fields.map((f) => f.key)).toEqual([
      'layer_height',
      'plugins.demo.enabled',
      'plugins.demo.amount',
    ]);
  });

  it('never emits a control for the namespace itself', () => {
    // The bag is a container, not a setting; rendering it would put a stray
    // text input labelled "Plugins" in the form.
    const { fields } = parseSchema(WITH_PLUGIN);
    expect(fields.find((f) => f.key === 'plugins')).toBeUndefined();
  });

  it('drops an open bag that declares no properties', () => {
    // What a build with no plugins emits.
    const { fields } = parseSchema({
      type: 'object',
      properties: {
        layer_height: { type: 'number' },
        plugins: { type: 'object', additionalProperties: true },
      },
    });
    expect(fields.map((f) => f.key)).toEqual(['layer_height']);
  });

  it('groups namespaced fields by their x-group like any other', () => {
    const { groups } = parseSchema(WITH_PLUGIN);
    const experiments = groups.find((g) => g.name === 'Experiments');
    expect(experiments?.fields.map((f) => f.key)).toEqual([
      'plugins.demo.enabled',
      'plugins.demo.amount',
    ]);
  });

  it('hides a plugin field until its own toggle is on', () => {
    // The whole reason plugin settings need no new UI concept: the existing
    // x-relevant-when machinery gates them, addressed by path.
    const { fields } = parseSchema(WITH_PLUGIN);
    const amount = fields.find((f) => f.key === 'plugins.demo.amount')!;
    expect(isFieldRelevant(amount, {})).toBe(false);
    expect(isFieldRelevant(amount, { plugins: { demo: { enabled: false } } })).toBe(false);
    expect(isFieldRelevant(amount, { plugins: { demo: { enabled: true } } })).toBe(true);
  });

  it('leaves a flat schema exactly as it was', () => {
    const { fields, groups } = parseSchema({
      type: 'object',
      required: ['layer_height'],
      properties: {
        layer_height: { type: 'number', title: 'Layer height', 'x-group': 'Layer', default: 0.2 },
        spiral_vase: { type: 'boolean', 'x-group': 'Quality' },
      },
    });
    expect(fields).toEqual([
      expect.objectContaining({
        key: 'layer_height',
        type: 'number',
        required: true,
        default: 0.2,
      }),
      expect.objectContaining({ key: 'spiral_vase', type: 'boolean', required: false }),
    ]);
    expect(groups.map((g) => g.name)).toEqual(['Layer', 'Quality']);
  });
});
