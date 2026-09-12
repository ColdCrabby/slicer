import { describe, expect, it } from 'vitest';
import { FILAMENT_MATERIALS, filamentTypeOptions } from './filament-types';

describe('filament type options', () => {
  it('offers the materials a printer expects to read back', () => {
    // The value lands in the G-code header as `; filament_type = …`, where
    // firmware and other slicers act on it — which is why this is a closed list.
    expect(FILAMENT_MATERIALS).toContain('PLA');
    expect(FILAMENT_MATERIALS).toContain('PETG');
    expect(FILAMENT_MATERIALS).toContain('ABS');
  });

  it('keeps a value the profile brought with it', () => {
    const options = filamentTypeOptions('PLA+');
    expect(options[0]).toMatchObject({ value: 'PLA+', description: 'From this filament profile' });
    expect(options.some((o) => o.value === 'PLA')).toBe(true);
  });

  it('does not duplicate a standard material', () => {
    expect(filamentTypeOptions('PETG').filter((o) => o.value === 'PETG')).toHaveLength(1);
  });

  it('offers only the standard list when there is no value yet', () => {
    expect(filamentTypeOptions('')).toHaveLength(FILAMENT_MATERIALS.length);
  });
});
