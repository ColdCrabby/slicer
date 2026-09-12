import type { SelectOption } from '@coldcrabby/ui';

/**
 * The materials offered for `filament_type`.
 *
 * These are the values other slicers and firmware expect to read back, which is
 * the whole reason this is a closed list rather than a text box: the value is
 * written into the G-code header as `; filament_type = …`, where a printer's
 * material detection and any downstream parser will act on it. A free-text
 * field let `TEST` reach that header — a value nothing on the receiving end can
 * do anything with.
 */
export const FILAMENT_MATERIALS = [
  'PLA',
  'PETG',
  'ABS',
  'ASA',
  'TPU',
  'PA',
  'PC',
  'PVA',
  'HIPS',
  'PEEK',
  'PP',
  'PET',
  'PCTG',
] as const;

/**
 * The options to offer for a current value.
 *
 * Kept in its own module, importing `SelectOption` as a *type* only, so the
 * rule below can be tested without resolving the component library.
 */
export function filamentTypeOptions(current: string): SelectOption[] {
  const options: SelectOption[] = FILAMENT_MATERIALS.map((m) => ({ value: m, label: m }));
  if (current && !FILAMENT_MATERIALS.some((m) => m === current)) {
    // Whatever the profile brought with it, kept selectable and labelled so the
    // user can see it is not one of the standard names. A vendor profile
    // carrying `PLA+` must not be rewritten just because the dropdown opened.
    options.unshift({ value: current, label: current, description: 'From this filament profile' });
  }
  return options;
}
