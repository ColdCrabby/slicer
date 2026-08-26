import { Type } from '@angular/core';
import { InfillDensitySlider } from '../custom-widgets/infill-density-slider/infill-density-slider';
import { InfillPatternPicker } from '../custom-widgets/infill-pattern-picker/infill-pattern-picker';
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
 * Key-specific widget overrides.
 * Add an entry here to swap in a custom widget for any schema field key.
 */
const KEY_REGISTRY: Record<string, Type<FieldWidget>> = {
  infill_density: InfillDensitySlider,
  infill_pattern: InfillPatternPicker,
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
 * Key-specific overrides take precedence over the type-based default.
 */
export function resolveWidget(field: FieldDef): Type<FieldWidget> {
  return KEY_REGISTRY[field.key] ?? defaultWidgetFor(field);
}
