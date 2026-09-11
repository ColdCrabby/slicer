import type { InputSignal } from '@angular/core';
import { EventEmitter } from '@angular/core';
import { FieldDef } from '../models/field-def';

/**
 * Common interface that all schema-form widget components must satisfy.
 * Both default widgets and custom overrides implement this contract.
 *
 * Angular's `input()` / `input.required()` returns an `InputSignal`,
 * so properties are typed accordingly.
 *
 * There is one markup obligation alongside it: **whatever element shows the
 * field's name must carry `class="field-label"`.** Widgets legitimately differ
 * on what that element is — a `<label>` where there is a single control to
 * point at, a `<span>` for a switch row or a radio group's legend — so the
 * container cannot find it by tag. It is how the settings panel marks a setting
 * the user changed away from their profile; a widget that omits it renders
 * correctly and silently stops being markable.
 */
export interface FieldWidget {
  field: InputSignal<FieldDef>;
  value: InputSignal<unknown>;
  valueChange: EventEmitter<unknown>;
}
