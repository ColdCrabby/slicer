import { Injectable, computed, effect, inject, signal } from '@angular/core';
import init, { GcodeHandle } from '../../generated/scene-wasm/scene_engine';
import { AppTheme } from './app-theme';
import { Slicer } from './slicer';

// ── Role palette — single source of truth shared by the viewer and controls ──

/** Keys that identify an extrusion role in the G-code viewer. */
export type RoleName =
  | 'outerWall'
  | 'innerWall'
  | 'infill'
  | 'topSurface'
  | 'bottomSurface'
  | 'travel'
  | 'other'
  | 'bridge'
  | 'overhangPerimeter'
  | 'skirt'
  | 'support'
  | 'seam';

/** Palette mapping every role to a numeric RGB hex color. */
export type RoleColorPalette = Record<RoleName, number>;

export const ROLE_COLORS_DARK: RoleColorPalette = {
  outerWall: 0xff8800, // amber-orange  (legend: outer wall)
  innerWall: 0xffcc00, // golden-yellow (legend: inner wall)
  infill: 0xcc44ff, // violet-purple (legend: sparse infill)
  topSurface: 0xff3355, // crimson-pink  (legend: top surface)
  bottomSurface: 0x00bbff, // vivid cyan    (legend: bottom surface)
  travel: 0x334466, // dark slate    (legend: travel)
  other: 0x44ffaa, // mint-green    (twist: stands apart)
  bridge: 0x0057ff, // vivid azure   (legend: bridge)
  overhangPerimeter: 0x008a4b, // emerald       (legend: overhang perimeter)
  skirt: 0x888888, // mid-gray      (legend: skirt/brim)
  support: 0x7dff00, // neon lime     (legend: support material)
  seam: 0xffffff, // white         (legend: seam point)
};

export const ROLE_COLORS_LIGHT: RoleColorPalette = {
  outerWall: 0xdd5500, // dark-orange    (legend: outer wall)
  innerWall: 0xbb8800, // deep-amber     (legend: inner wall)
  infill: 0x9900cc, // deep-violet    (legend: sparse infill)
  topSurface: 0xcc0033, // dark-crimson   (legend: top surface)
  bottomSurface: 0x0077bb, // ocean-blue     (legend: bottom surface)
  travel: 0x445566, // dark-slate     (legend: travel)
  other: 0x008855, // forest-teal    (twist: stands apart)
  bridge: 0x0044cc, // dark-azure     (legend: bridge)
  overhangPerimeter: 0x005e30, // deep-green     (legend: overhang perimeter)
  skirt: 0x666666, // dark-gray      (legend: skirt/brim)
  support: 0x557700, // dark-lime      (legend: support material)
  seam: 0x111122, // near-black     (legend: seam point — white bg)
};

/** Returns the correct palette for the current theme. */
export function getRoleColors(isDark: boolean): RoleColorPalette {
  return isDark ? ROLE_COLORS_DARK : ROLE_COLORS_LIGHT;
}

/** @deprecated Use `ROLE_COLORS_DARK` or `getRoleColors()` instead. */
export const ROLE_COLORS = ROLE_COLORS_DARK;

export const ROLE_LABELS: Record<RoleName, string> = {
  outerWall: 'Outer Wall',
  innerWall: 'Inner Wall',
  infill: 'Infill',
  topSurface: 'Top Surface',
  bottomSurface: 'Bottom Surface',
  travel: 'Travel',
  other: 'Other',
  bridge: 'Bridge',
  overhangPerimeter: 'Overhang Perimeter',
  skirt: 'Skirt / Brim',
  support: 'Support',
  seam: 'Seam',
};

function makeRoleCss(colors: RoleColorPalette): Record<RoleName, string> {
  return Object.fromEntries(
    Object.entries(colors).map(([k, v]) => [k, `#${v.toString(16).padStart(6, '0')}`]),
  ) as Record<RoleName, string>;
}

export const ROLE_CSS_DARK = makeRoleCss(ROLE_COLORS_DARK);
export const ROLE_CSS_LIGHT = makeRoleCss(ROLE_COLORS_LIGHT);

/** @deprecated Use `ROLE_CSS_DARK` or `GcodePreview.roleCss` signal instead. */
export const ROLE_CSS = ROLE_CSS_DARK;

export const ROLE_ORDER: readonly RoleName[] = [
  'outerWall',
  'innerWall',
  'infill',
  'topSurface',
  'bottomSurface',
  'bridge',
  'overhangPerimeter',
  'skirt',
  'support',
  'travel',
  'seam',
  'other',
] as const;

// ── Segment buffer layout ───────────────────────────────────────────────────

/**
 * Number of `f32`s per line-segment record in a `GcodeLayerBuffer` block:
 * `[x0, y0, z0, x1, y1, z1, width, height, speed]`. Kept in one place so the
 * WASM buffer stride and every TypeScript reader stay in sync.
 */
export const FLOATS_PER_SEGMENT = 9;

/** Byte offset (in floats) of the per-segment extrusion speed (mm/s). */
export const SPEED_OFFSET = 8;

// ── View mode + speed coloring ──────────────────────────────────────────────

/**
 * How the viewer colors extrusion segments.
 * - `category` (default): by extrusion role — outer wall, infill, and so on.
 * - `speed`: by extrusion feedrate, mapped through {@link SPEED_GRADIENT_STOPS}.
 */
export type GcodeViewMode = 'category' | 'speed';

/** Human-readable labels for the view-mode dropdown. */
export const VIEW_MODE_LABELS: Record<GcodeViewMode, string> = {
  category: 'Categories',
  speed: 'Speed',
};

/** Extrusion-speed range (mm/s) of the current model, slow → fast. */
export interface SpeedRange {
  min: number;
  max: number;
}

/**
 * Slow → fast color ramp for the speed view (blue → cyan → green → amber → red).
 * Shared by the 3D renderer (`sampleSpeedColor`) and the legend gradient
 * (`speedGradientCss`) so both stay identical.
 */
export const SPEED_GRADIENT_STOPS: readonly number[] = [
  0x3b4cc0, 0x00b4d8, 0x2dc937, 0xf9c80e, 0xe63946,
] as const;

/** Sample the speed ramp at `t` ∈ [0, 1], returning a packed `0xRRGGBB` color. */
export function sampleSpeedColor(t: number): number {
  const stops = SPEED_GRADIENT_STOPS;
  const clamped = Math.min(1, Math.max(0, Number.isFinite(t) ? t : 0));
  const scaled = clamped * (stops.length - 1);
  const i = Math.min(stops.length - 2, Math.floor(scaled));
  const f = scaled - i;
  const c0 = stops[i];
  const c1 = stops[i + 1];
  const lerp = (a: number, b: number) => Math.round(a + (b - a) * f);
  const r = lerp((c0 >> 16) & 0xff, (c1 >> 16) & 0xff);
  const g = lerp((c0 >> 8) & 0xff, (c1 >> 8) & 0xff);
  const b = lerp(c0 & 0xff, c1 & 0xff);
  return (r << 16) | (g << 8) | b;
}

/** CSS `linear-gradient(...)` mirroring the speed ramp; `to right` = slow → fast. */
export function speedGradientCss(direction = 'to right'): string {
  const n = SPEED_GRADIENT_STOPS.length;
  const stops = SPEED_GRADIENT_STOPS.map(
    (c, i) => `#${c.toString(16).padStart(6, '0')} ${((i / (n - 1)) * 100).toFixed(0)}%`,
  );
  return `linear-gradient(${direction}, ${stops.join(', ')})`;
}

// Role ids that are not extrusions and so are excluded from the speed range.
const TRAVEL_ROLE_ID = 5;
const SEAM_ROLE_ID = 10;

/** Scan every extruding segment of a parsed handle for its min/max speed (mm/s). */
function computeSpeedRange(handle: GcodeHandle): SpeedRange {
  let min = Infinity;
  let max = 0;
  const layerCount = handle.layerCount();
  for (let li = 0; li < layerCount; li++) {
    const layer = handle.getLayer(li);
    const blockCount = layer.blocksCount();
    for (let b = 0; b < blockCount; b++) {
      const roleId = layer.blockRole(b);
      if (roleId === TRAVEL_ROLE_ID || roleId === SEAM_ROLE_ID) {
        continue;
      }
      const data = layer.blockData(b);
      for (let o = SPEED_OFFSET; o < data.length; o += FLOATS_PER_SEGMENT) {
        const s = data[o];
        if (s > 0) {
          if (s < min) min = s;
          if (s > max) max = s;
        }
      }
    }
  }
  return Number.isFinite(min) ? { min, max } : { min: 0, max: 0 };
}

// ── Service ───────────────────────────────────────────────────────────────────

/**
 * Owns the parsed `GcodeHandle` for the current slice session and exposes
 * reactive signals consumed by both the layer/segment control components and
 * the `Viewer` gcode rendering path.
 *
 * When `showAllLayers` is `true` (default) all layers from 0 to `layerMax` are
 * rendered, giving a cumulative "layers built so far" view.  When `false` only
 * the single layer at `layerMax` is shown.
 */
@Injectable({ providedIn: 'root' })
export class GcodePreview {
  private readonly slicer = inject(Slicer);
  private readonly appTheme = inject(AppTheme);

  /** Parsed handle — `null` until a slice download URL is available. */
  readonly gcodeHandle = signal<GcodeHandle | null>(null);

  /** Active role color palette — switches with the current theme. */
  readonly roleColors = computed<RoleColorPalette>(() => getRoleColors(this.appTheme.isDarkMode()));

  /** Active CSS color map for legend/controls — switches with the current theme. */
  readonly roleCss = computed<Record<RoleName, string>>(() =>
    this.appTheme.isDarkMode() ? ROLE_CSS_DARK : ROLE_CSS_LIGHT,
  );

  /** `true` while bytes are being fetched / parsed. */
  readonly loading = signal(false);

  /** Derived total layer count. */
  readonly layerCount = computed(() => this.gcodeHandle()?.layerCount() ?? 0);

  /**
   * Upper bound of the visible layer range (0-based index).
   * The single moving thumb on the vertical layer scrollbar.
   */
  readonly layerMax = signal(0);

  /**
   * When `true` (default) all layers from 0 up to `layerMax` are rendered.
   * When `false` only the single layer at `layerMax` is shown.
   */
  readonly showAllLayers = signal(true);

  /**
   * Lower bound of the visible layer range (0-based index).
   * Derived: always 0 when `showAllLayers` is true, otherwise equals `layerMax`.
   */
  readonly layerMin = computed(() => (this.showAllLayers() ? 0 : this.layerMax()));

  /**
   * Fractional scrub position within the top-most visible layer [0, 1].
   * 0 = nothing shown; 1 = full layer revealed.
   * Automatically resets to 1 when navigating to a different layer via `setLayerMax`.
   */
  readonly segmentProgress = signal(1);

  /** Set of roles to hide in the viewer. */
  readonly hiddenRoles = signal<ReadonlySet<RoleName>>(new Set<RoleName>());

  /** Active coloring mode: role categories (default) or extrusion speed. */
  readonly viewMode = signal<GcodeViewMode>('category');

  /** Extrusion-speed range (mm/s) of the loaded model, for the speed legend. */
  readonly speedRange = signal<SpeedRange>({ min: 0, max: 0 });

  constructor() {
    // React to every new download URL produced by the slicer service.
    effect(() => {
      const url = this.slicer.gcodeDownloadUrl();
      if (!url) {
        return;
      }
      void this.#loadFromUrl(url);
    });
  }

  // ── Mutators ────────────────────────────────────────────────────────────

  setLayerMax(value: number): void {
    const count = this.layerCount();
    if (count === 0) {
      return;
    }
    const clamped = Math.max(0, Math.min(value, count - 1));
    // Reset segment scrub to fully-revealed whenever the active layer changes
    // so the thumb doesn't visually jump as the new layer's segment count differs.
    if (clamped !== this.layerMax()) {
      this.segmentProgress.set(1);
    }
    this.layerMax.set(clamped);
  }

  setSegmentProgress(value: number): void {
    this.segmentProgress.set(Math.max(0, Math.min(1, value)));
  }

  toggleRole(role: RoleName): void {
    const current = this.hiddenRoles();
    const next = new Set(current);
    if (next.has(role)) {
      next.delete(role);
    } else {
      next.add(role);
    }
    this.hiddenRoles.set(next);
  }

  toggleShowAllLayers(): void {
    this.showAllLayers.set(!this.showAllLayers());
  }

  setViewMode(mode: GcodeViewMode): void {
    this.viewMode.set(mode);
  }

  // ── Private ──────────────────────────────────────────────────────────────

  async #loadFromUrl(url: string): Promise<void> {
    this.loading.set(true);
    this.gcodeHandle.set(null);
    try {
      await init({ module_or_path: 'scene_engine_bg.wasm' });
      const response = await fetch(url);
      const buffer = await response.arrayBuffer();
      const handle = GcodeHandle.parse(new Uint8Array(buffer));
      const count = handle.layerCount();
      this.speedRange.set(computeSpeedRange(handle));
      this.gcodeHandle.set(handle);
      this.layerMax.set(Math.max(0, count - 1));
      this.segmentProgress.set(1);
      this.hiddenRoles.set(new Set<RoleName>());
      this.showAllLayers.set(true);
      this.viewMode.set('category');
    } catch (error) {
      console.error('[GcodePreview] Failed to load gcode:', error);
    } finally {
      this.loading.set(false);
    }
  }
}
