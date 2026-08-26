import type { InputSignal } from '@angular/core';
import { EventEmitter } from '@angular/core';
import { FieldDef } from '../models/field-def';

/**
 * Common interface that all schema-form widget components must satisfy.
 * Both default widgets and custom overrides implement this contract.
 *
 * Angular's `input()` / `input.required()` returns an `InputSignal`,
 * so properties are typed accordingly.
 */
export interface FieldWidget {
  field: InputSignal<FieldDef>;
  value: InputSignal<unknown>;
  valueChange: EventEmitter<unknown>;
}
