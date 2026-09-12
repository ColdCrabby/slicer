import type { FieldDef } from './field-def';

/**
 * The controls a schema field may render as — **the whole set**, deliberately
 * short. Every settings surface picks from this list and nothing else, so the
 * same parameter looks the same in the slice sidebar as in the profile editors.
 *
 * | Kind | Used for | Why this one |
 * | --- | --- | --- |
 * | `switch` | every boolean | on/off is not a choice between two things |
 * | `segmented` | an enum of up to {@link SEGMENTED_MAX_OPTIONS} plain choices | all options visible, one line, no explanation needed |
 * | `cards` | an enum whose branches work differently enough to need a sentence each | the description is the point, so it has to be on screen |
 * | `select` | a longer enum, or an open list | a list you pick from, not a set of modes |
 * | `number` | every numeric parameter | the unit and step come from `x-unit` / `x-step` |
 * | `slider` | a number whose *feel* matters more than its digits | infill density |
 * | `text`, `color`, `gcode`, `filament-type` | the string shapes | a colour is a swatch, G-code is an editor |
 * | `array` | structured lists (fan curves, triggers) | has an editor of its own; the form skips it |
 *
 * The judgement that keeps this coherent is **`segmented` vs `cards`**, and it
 * is not about how many options there are. Ask whether the user can tell the
 * options apart from their names alone. `Light · Dark · Transparent` they can,
 * so it is a segmented control. `Classic` vs `Arachne` they cannot, and
 * choosing wrong changes every wall on the print — that earns cards, and the
 * room for a line of explanation under each one.
 *
 * Because the second half of that question is a judgement about the *setting*,
 * it is answered in the schema (`x-widget = "cards"`) rather than inferred
 * here. Option count only decides between `segmented` and `select`.
 */
export type ControlKind =
  | 'switch'
  | 'segmented'
  | 'cards'
  | 'select'
  | 'number'
  | 'slider'
  | 'text'
  | 'color'
  | 'gcode'
  | 'filament-type'
  | 'array';

/**
 * Most options a segmented control is asked to hold. Beyond this the segments
 * are too narrow to read at sidebar width even stacked, and the choice reads
 * better as a list.
 */
export const SEGMENTED_MAX_OPTIONS = 3;

/**
 * `x-widget` hints the schema may set, mapped to the control they select.
 * A hint overrides the shape-derived default — it is how a field says "this
 * decision is bigger than its type suggests".
 */
const WIDGET_HINTS: Record<string, ControlKind> = {
  gcode: 'gcode',
  cards: 'cards',
  segmented: 'segmented',
  select: 'select',
  color: 'color',
};

/**
 * Field keys whose control cannot be derived from the schema at all, because
 * what makes them special is what the value *means*.
 */
const KEY_CONTROLS: Record<string, ControlKind> = {
  // A percentage the user dials in by feel, against a live preview.
  infill_density: 'slider',
  // A closed list, not a text box: the value is written into the G-code header
  // as `; filament_type = …`, where firmware and other slicers read it back.
  filament_type: 'filament-type',
  // A colour is a colour wherever it appears. Without these the filament's own
  // swatch was a text box holding `#RRGGBB`.
  filament_color: 'color',
  thumbnail_custom_color: 'color',
};

/**
 * The control a field renders as. Precedence: the key's own override, then the
 * schema's `x-widget` hint, then the field's shape.
 *
 * Exhaustive on purpose. An earlier default returned a number input for
 * everything it did not recognise, which is how a filament's name, type and
 * colour all became number spinners.
 */
export function controlFor(field: FieldDef): ControlKind {
  const explicit =
    KEY_CONTROLS[field.key] ?? (field.widget ? WIDGET_HINTS[field.widget] : undefined);
  if (explicit) {
    return explicit;
  }

  if (field.enumOptions?.length) {
    return field.enumOptions.length <= SEGMENTED_MAX_OPTIONS ? 'segmented' : 'select';
  }

  switch (field.type) {
    case 'boolean':
      return 'switch';
    case 'integer':
    case 'number':
      return 'number';
    case 'array':
      return 'array';
    default:
      return 'text';
  }
}
