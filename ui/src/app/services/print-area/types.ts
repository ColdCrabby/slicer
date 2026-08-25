/**
 * Shared shapes for the {@link ./print-area `PrintArea`}.
 *
 * Object identity, position, rotation, and scale live with
 * {@link ../object-tracker `ObjectTracker`}; this file only carries
 * configuration + UI-helper types.
 */

/**
 * Description of the printer's build volume in machine coordinates.
 *
 * The viewer's world origin (0, 0, 0) — where the RGB axis gizmo sits — is
 * always the printer's machine origin. The build plate ("printable area") can
 * be rectangular or circular ({@link bedShape}). Dimensions are given by
 * {@link printableAreaWidth} / {@link printableAreaHeight} and positioned by
 * ({@link movableAreaX}, {@link movableAreaY}). For circular beds, width/height
 * describe the bounding box and are usually equal to the diameter.
 *
 * This separation lets us model real-world printers correctly: many machines
 * can drive their toolhead to coordinates that lie outside the physical bed
 * (e.g. for purge towers, wipe positions, parking) so the bed itself does
 * not have to start at (0, 0).
 */
export interface PrintAreaConfig {
  /** Shape of the printable area. */
  bedShape: 'rectangular' | 'circular';
  /** Width of the bed in millimetres (along the world +X axis). */
  printableAreaWidth: number;
  /** Depth of the bed in millimetres (along the world +Y axis). */
  printableAreaHeight: number;
  /** X offset of the bed's lower-left corner from the machine origin (mm). */
  movableAreaX: number;
  /** Y offset of the bed's lower-left corner from the machine origin (mm). */
  movableAreaY: number;
}

/** Snapshot of a single object's XY position at the moment a drag began. */
export interface DragAnchor {
  id: string;
  startX: number;
  startY: number;
}

/** Options accepted by `PrintArea.select`. */
export interface SelectOptions {
  /**
   * When `true`, the id is added to (or removed from, if already present)
   * the existing selection — the ctrl/⌘+click semantics. When `false`
   * (default) the selection is replaced with just this id.
   */
  additive?: boolean;
}

/** Resolved bed bounds in machine coordinates. */
export interface PrintAreaBounds {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
  centerX: number;
  centerY: number;
}

/** Default print-area configuration applied on first run / reset. */
export const DEFAULT_PRINT_AREA_CONFIG: PrintAreaConfig = {
  bedShape: 'rectangular',
  printableAreaWidth: 220,
  printableAreaHeight: 220,
  movableAreaX: 0,
  movableAreaY: 0,
};
