import { DEFAULT_PRINT_AREA_CONFIG, PrintAreaConfig } from './types';

/** Coerce a numeric input to a positive finite value, or fall back. */
export function ensurePositive(value: number, fallback: number): number {
  return Number.isFinite(value) && value > 0 ? value : fallback;
}

/** Coerce a numeric input to a finite value, or fall back. */
export function ensureFinite(value: number, fallback: number): number {
  return Number.isFinite(value) ? value : fallback;
}

/**
 * Sanitise an arbitrary {@link PrintAreaConfig} payload — clamping each
 * field to the constraints the rest of the codebase relies on (positive
 * dimensions, finite offsets) and substituting sensible defaults when a
 * value is missing or invalid.
 */
export function sanitisePrintAreaConfig(next: PrintAreaConfig): PrintAreaConfig {
  const bedShape = next.bedShape === 'circular' ? 'circular' : 'rectangular';
  return {
    bedShape,
    printableAreaWidth: ensurePositive(
      next.printableAreaWidth,
      DEFAULT_PRINT_AREA_CONFIG.printableAreaWidth,
    ),
    printableAreaHeight: ensurePositive(
      next.printableAreaHeight,
      DEFAULT_PRINT_AREA_CONFIG.printableAreaHeight,
    ),
    movableAreaX: ensureFinite(next.movableAreaX, 0),
    movableAreaY: ensureFinite(next.movableAreaY, 0),
  };
}
