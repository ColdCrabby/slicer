import { Type } from '@angular/core';
import { ColorField } from '../custom-widgets/color-field/color-field';
import { GcodeField } from '../custom-widgets/gcode-field/gcode-field';
import { InfillDensitySlider } from '../custom-widgets/infill-density-slider/infill-density-slider';
import { FieldDef } from '../models/field-def';
import { FieldWidget } from '../widgets/base-field';
import { BooleanField } from '../widgets/boolean-field/boolean-field';
import { EnumRadio } from '../widgets/enum-radio/enum-radio';
import { EnumSelect } from '../widgets/enum-select/enum-select';
import { IntegerField } from '../widgets/integer-field/integer-field';
import { NumberField } from '../widgets/number-field/number-field';

/**
 * Maximum number of enum options for which a radio group is used.
 * Fields with more options than this threshold render as a `<select>` dropdown.
 */
const RADIO_MAX_OPTIONS = 3;

/**
 * `x-widget`-driven widget overrides, keyed by the schema's `x-widget` hint.
 * A field carrying one of these hints renders its mapped widget regardless of
 * key or type — the schema is the single source of truth for "this field needs
 * a special control".
 */
const WIDGET_REGISTRY: Record<string, Type<FieldWidget>> = {
  gcode: GcodeField,
};

/**
 * Key-specific widget overrides.
 * Add an entry here to swap in a custom widget for any schema field key.
 */
const KEY_REGISTRY: Record<string, Type<FieldWidget>> = {
  infill_density: InfillDensitySlider,
  // `infill_pattern` deliberately has **no** override: it falls through to the
  // enum default (a dropdown, since it has far more than RADIO_MAX_OPTIONS
  // choices) so every pattern the engine offers is selectable and carries its
  // schema description. A hand-written picker here previously hard-coded five
  // of them, which silently hid the rest as the engine grew.
  thumbnail_custom_color: ColorField,
};

/**
 * Select the default widget for a field based on its type and enum cardinality.
 */
function defaultWidgetFor(field: FieldDef): Type<FieldWidget> {
  if (field.enumOptions) {
    return field.enumOptions.length <= RADIO_MAX_OPTIONS ? EnumRadio : EnumSelect;
  }

  switch (field.type) {
    case 'integer':
      return IntegerField;
    case 'boolean':
      return BooleanField;
    default:
      return NumberField;
  }
}

/**
 * Resolve the widget component class for a given field.
 * Precedence: key-specific override → `x-widget` hint → type-based default.
 */
export function resolveWidget(field: FieldDef): Type<FieldWidget> {
  return (
    KEY_REGISTRY[field.key] ??
    (field.widget ? WIDGET_REGISTRY[field.widget] : undefined) ??
    defaultWidgetFor(field)
  );
}
