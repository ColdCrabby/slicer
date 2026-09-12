import { Type } from '@angular/core';
import { ColorField } from '../custom-widgets/color-field/color-field';
import { FilamentTypeField } from '../custom-widgets/filament-type-field/filament-type-field';
import { GcodeField } from '../custom-widgets/gcode-field/gcode-field';
import { InfillDensitySlider } from '../custom-widgets/infill-density-slider/infill-density-slider';
import { type ControlKind, controlFor } from '../models/field-control';
import { FieldDef } from '../models/field-def';
import { FieldWidget } from '../widgets/base-field';
import { BooleanField } from '../widgets/boolean-field/boolean-field';
import { EnumCards } from '../widgets/enum-cards/enum-cards';
import { EnumSegmented } from '../widgets/enum-segmented/enum-segmented';
import { EnumSelect } from '../widgets/enum-select/enum-select';
import { IntegerField } from '../widgets/integer-field/integer-field';
import { NumberField } from '../widgets/number-field/number-field';
import { TextField } from '../widgets/text-field/text-field';

/**
 * The slice sidebar's rendering of the shared control taxonomy: one component
 * per {@link ControlKind}.
 *
 * **Which control a field gets is not decided here** — {@link controlFor} owns
 * that, and the profile editors ask it the same question, which is what stops a
 * parameter being option cards in the sidebar and a dropdown in Settings. This
 * map only says what each kind looks like on this surface.
 *
 * `array` has no entry: fan curves and pause triggers are structured lists with
 * editors of their own, and the panel filters them out before it gets here.
 * Falling through to a widget would hand a list of objects to a number spinner.
 */
const WIDGETS: Partial<Record<ControlKind, Type<FieldWidget>>> = {
  switch: BooleanField,
  segmented: EnumSegmented,
  cards: EnumCards,
  // `infill_pattern` and the surface patterns land here on option count alone,
  // so every pattern the engine offers stays selectable and carries its schema
  // description. A hand-written picker once hard-coded five of them, which
  // silently hid the rest as the engine grew.
  select: EnumSelect,
  number: NumberField,
  slider: InfillDensitySlider,
  text: TextField,
  color: ColorField,
  gcode: GcodeField,
  'filament-type': FilamentTypeField,
};

/**
 * Resolve the widget component class for a given field.
 *
 * Integers keep their own widget rather than sharing the number one, because
 * the two differ in step and in what they will accept typed into them.
 */
export function resolveWidget(field: FieldDef): Type<FieldWidget> {
  const kind = controlFor(field);
  if (kind === 'number' && field.type === 'integer') {
    return IntegerField;
  }
  return WIDGETS[kind] ?? TextField;
}
