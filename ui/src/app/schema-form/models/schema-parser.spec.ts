import { parseSchema } from './schema-parser';
import { isFieldRelevant } from './relevance';
import globalSettingsSchema from '../../../schemas/slicer-engine-global-settings-v1.json';

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

describe('the schema the engine actually generates', () => {
  // Guards the seam between the engine's schema injection and this parser. A
  // synthetic fixture cannot catch the two of them disagreeing about shape —
  // and they did: the injector stopped at the wrapper's own `properties` and
  // silently put the plugin fragment nowhere.
  const schema = globalSettingsSchema.$defs.SlicingParams as unknown as Record<string, unknown>;

  it('renders every shipped plugin as grouped, gated fields', () => {
    const { fields } = parseSchema(schema);
    const pluginFields = fields.filter((f) => f.key.startsWith('plugins.'));

    expect(pluginFields.length).toBeGreaterThan(0);
    for (const field of pluginFields) {
      expect(field.key.split('.').length).toBe(3);
      expect(field.group).toBe('Experiments');

      const isToggle = field.key.endsWith('.enabled');
      if (isToggle) {
        // The plugin's own gate is always visible; everything else hangs off it.
        expect(field.type).toBe('boolean');
        expect(field.relevantWhen).toBeUndefined();
      } else {
        const namespace = field.key.split('.').slice(0, 2).join('.');
        expect(field.relevantWhen).toEqual({
          field: `${namespace}.enabled`,
          equals: true,
          greaterThan: undefined,
        });
      }
    }
  });

  it('hides a plugin\u2019s settings until it is switched on', () => {
    const { fields } = parseSchema(schema);
    const gated = fields.find((f) => f.key.startsWith('plugins.') && !f.key.endsWith('.enabled'));
    expect(gated).toBeDefined();

    const namespace = gated!.key.split('.')[1];
    expect(isFieldRelevant(gated!, {})).toBe(false);
    expect(isFieldRelevant(gated!, { plugins: { [namespace]: { enabled: true } } })).toBe(true);
  });

  it('never emits a control for the plugins bag itself', () => {
    const { fields } = parseSchema(schema);
    expect(fields.find((f) => f.key === 'plugins')).toBeUndefined();
  });
});
