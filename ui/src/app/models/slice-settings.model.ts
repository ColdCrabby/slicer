export type { SlicingParams as SliceSettings } from '../../generated/slicer-engine-ws-client-message-v1';

const THUMBNAIL_DEFAULTS: Record<string, unknown> = {
  thumbnail_enabled: true,
  thumbnail_size_px: 320,
  thumbnail_view: 'isometric',
  thumbnail_theme: 'transparent',
  thumbnail_color_mode: 'filament',
  thumbnail_custom_color: '#e0912f',
};

export const DEFAULT_SETTINGS: import('../../generated/slicer-engine-ws-client-message-v1').SlicingParams =
  {
    layer_height: 0.2,
    print_speed: 60,
    nozzle_temp: 215,
    bed_temp: 60,
    gcode_flavor: 'marlin',
    // SlicingParams.infill_density is a fraction 0.0–1.0 — NOT a percentage.
    // The previous WS-only struct expected 0–100 and divided by 100 server-side,
    // which silently broke the moment the settings panel started writing the
    // SlicingParams form value (e.g. 0.3 → 0.003 → no infill).
    infill_density: 0.2,
    infill_pattern: 'Rectilinear',
    infill_base_angle: 45,
    ...THUMBNAIL_DEFAULTS,
  };
