import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
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
  outerWall: 0xe0620c, // warm orange   (legend: outer wall)
  innerWall: 0xc08800, // amber-gold     (legend: inner wall)
  infill: 0x8e3fc4, // medium violet   (legend: sparse infill)
  topSurface: 0xd1263f, // rose-crimson    (legend: top surface)
  bottomSurface: 0x1592c4, // azure-cyan      (legend: bottom surface)
  travel: 0x8a94a6, // muted slate     (legend: travel)
  other: 0x0f9f97, // teal            (twist: stands apart)
  bridge: 0x2e5bd6, // royal blue      (legend: bridge)
  overhangPerimeter: 0x1e9e62, // emerald green   (legend: overhang perimeter)
  skirt: 0x74787f, // neutral grey    (legend: skirt/brim)
  support: 0x7f9c1f, // olive-lime      (legend: support material)
  seam: 0x2a2e38, // dark slate      (legend: seam point \u2014 white bg)
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

const DEFAULT_HIDDEN_ROLES: ReadonlySet<RoleName> = new Set<RoleName>(['travel', 'seam']);

// ── Segment buffer layout ───────────────────────────────────────────────────

/**
 * Number of `f32`s per line-segment record in a `GcodeLayerBuffer` block:
 * `[x0, y0, z0, x1, y1, z1, width, height, speed]`. Kept in one place so the
 * WASM buffer stride and every TypeScript reader stay in sync.
 */
export const FLOATS_PER_SEGMENT = 9;

/** Float offset of the per-segment extrusion width (mm). */
export const WIDTH_OFFSET = 6;
/** Float offset of the per-segment layer height (mm). */
export const HEIGHT_OFFSET = 7;
/** Float offset of the per-segment extrusion speed (mm/s). */
export const SPEED_OFFSET = 8;

// ── View modes + scalar coloring ────────────────────────────────────────────

/**
 * How the viewer colors extrusion segments.
 * - `category` (default): by extrusion role — outer wall, infill, and so on.
 * - *segment* scalars (`speed`, `flow`, `lineWidth`, `layerHeight`) vary per
 *   move and are derived from the segment's width/height/speed.
 * - *layer* scalars (`fan`, `temperature`, `layerTime`) are one value per layer,
 *   read from the layer's machine-state metadata.
 * Every scalar is mapped through the shared gradient ramp.
 */
export type SegmentViewMode = 'speed' | 'flow' | 'lineWidth' | 'layerHeight';
export type LayerViewMode = 'fan' | 'temperature' | 'layerTime';
export type GcodeViewMode = 'category' | SegmentViewMode | LayerViewMode;

/** Ordered list backing the "Color by" dropdown (before availability filtering). */
export const VIEW_MODE_ORDER: readonly GcodeViewMode[] = [
  'category',
  'speed',
  'flow',
  'lineWidth',
  'layerHeight',
  'fan',
  'temperature',
  'layerTime',
] as const;

/** Human-readable labels for the view-mode dropdown. */
export const VIEW_MODE_LABELS: Record<GcodeViewMode, string> = {
  category: 'Categories',
  speed: 'Speed',
  flow: 'Flow',
  lineWidth: 'Line Width',
  layerHeight: 'Layer Height',
  fan: 'Fan Speed',
  temperature: 'Temperature',
  layerTime: 'Layer Time',
};

/** Min/max span of a scalar channel across the loaded model, low → high. */
export interface ScalarRange {
  min: number;
  max: number;
}

/** @deprecated Use {@link ScalarRange}. */
export type SpeedRange = ScalarRange;

/** Per-layer machine state consumed by the layer-scalar color channels. */
export interface LayerScalarMeta {
  /** Nozzle target temperature (°C); `0` when unknown. */
  nozzleTemp: number;
  /** Active tool / extruder index. */
  tool: number;
  /** Layer print time (s); `0` when unknown. */
  layerTimeS: number;
  /** Fan speeds keyed by stable id (`"P0"`, `"fan_chamber"`, …), `0..1`. */
  fans: ReadonlyMap<string, number>;
}

/**
 * A per-segment scalar channel. `extract` derives the value from a segment's
 * width/height/speed triple, keeping the range scan and renderer independent of
 * the raw WASM buffer layout.
 */
export interface ScalarChannel {
  id: SegmentViewMode;
  scope: 'segment';
  label: string;
  unit: string;
  extract: (width: number, height: number, speed: number) => number;
  format: (value: number) => string;
}

/**
 * A per-layer scalar channel (fan / temperature / layer time). `extractLayer`
 * returns the layer's value, or `null` when that layer has no data for it.
 * `needsFanParam` channels are parameterized by the selected fan key.
 */
export interface LayerChannel {
  id: LayerViewMode;
  scope: 'layer';
  label: string;
  unit: string;
  needsFanParam?: boolean;
  extractLayer: (meta: LayerScalarMeta, param: string | null) => number | null;
  format: (value: number) => string;
}

export type ColorChannel = ScalarChannel | LayerChannel;

const roundFmt =
  (unit: string) =>
  (v: number): string =>
    v > 0 ? `${Math.round(v)} ${unit}` : '—';
const fixed2Fmt =
  (unit: string) =>
  (v: number): string =>
    v > 0 ? `${v.toFixed(2)} ${unit}` : '—';

/**
 * Registry of per-segment scalar channels. Adding a segment mode is a matter of
 * registering one entry here plus listing it in {@link VIEW_MODE_ORDER}.
 */
export const SCALAR_CHANNELS: Record<SegmentViewMode, ScalarChannel> = {
  speed: {
    id: 'speed',
    scope: 'segment',
    label: 'Speed',
    unit: 'mm/s',
    extract: (_w, _h, s) => s,
    format: roundFmt('mm/s'),
  },
  flow: {
    id: 'flow',
    scope: 'segment',
    label: 'Flow',
    unit: 'mm³/s',
    // Approximate volumetric rate: cross-section (w×h) × linear speed.
    extract: (w, h, s) => w * h * s,
    format: fixed2Fmt('mm³/s'),
  },
  lineWidth: {
    id: 'lineWidth',
    scope: 'segment',
    label: 'Line Width',
    unit: 'mm',
    extract: (w) => w,
    format: fixed2Fmt('mm'),
  },
  layerHeight: {
    id: 'layerHeight',
    scope: 'segment',
    label: 'Layer Height',
    unit: 'mm',
    extract: (_w, h) => h,
    format: fixed2Fmt('mm'),
  },
};

/** Registry of per-layer scalar channels (fan / temperature / layer time). */
export const LAYER_CHANNELS: Record<LayerViewMode, LayerChannel> = {
  fan: {
    id: 'fan',
    scope: 'layer',
    label: 'Fan Speed',
    unit: '%',
    needsFanParam: true,
    extractLayer: (m, param) => (param != null ? (m.fans.get(param) ?? null) : null),
    format: (v) => (v >= 0 ? `${Math.round(v * 100)}%` : '—'),
  },
  temperature: {
    id: 'temperature',
    scope: 'layer',
    label: 'Temperature',
    unit: '°C',
    extractLayer: (m) => (m.nozzleTemp > 0 ? m.nozzleTemp : null),
    format: roundFmt('°C'),
  },
  layerTime: {
    id: 'layerTime',
    scope: 'layer',
    label: 'Layer Time',
    unit: 's',
    extractLayer: (m) => (m.layerTimeS > 0 ? m.layerTimeS : null),
    format: (v) => (v > 0 ? `${v.toFixed(v < 10 ? 1 : 0)} s` : '—'),
  },
};

/** Resolve the color channel for a mode, or `null` for the categorical mode. */
export function scalarChannelFor(mode: GcodeViewMode): ColorChannel | null {
  if (mode === 'category') {
    return null;
  }
  return mode in SCALAR_CHANNELS
    ? SCALAR_CHANNELS[mode as SegmentViewMode]
    : LAYER_CHANNELS[mode as LayerViewMode];
}

/** Human label for a fan key (`"P0"` → "Part Cooling", `"fan_chamber"` → "Chamber"). */
export function fanLabel(key: string): string {
  const marlin = /^P(\d+)$/.exec(key);
  if (marlin) {
    const idx = Number(marlin[1]);
    return ['Part Cooling', 'Hotend', 'Chamber', 'Aux'][idx] ?? `Fan ${idx}`;
  }
  const klipper: Record<string, string> = {
    fan: 'Part Cooling',
    fan_hotend: 'Hotend',
    fan_chamber: 'Chamber',
    fan_aux: 'Aux',
  };
  return klipper[key] ?? key;
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

// Role ids that are not extrusions and so are excluded from scalar ranges.
const TRAVEL_ROLE_ID = 5;
const SEAM_ROLE_ID = 10;

/** A fan discovered in the loaded model, with its speed range across layers. */
export interface FanInfo {
  key: string;
  label: string;
  range: ScalarRange;
}

/** Aggregated scalar metadata for the loaded model, computed in one scan. */
export interface ModelScan {
  segmentRanges: Record<SegmentViewMode, ScalarRange>;
  temperature: ScalarRange;
  layerTime: ScalarRange;
  /** Fans discovered across all layers, in first-seen order. */
  fans: FanInfo[];
}

/** Zeroed scan for an empty / not-yet-loaded model. */
export function emptyModelScan(): ModelScan {
  const segmentRanges = {} as Record<SegmentViewMode, ScalarRange>;
  for (const id of Object.keys(SCALAR_CHANNELS) as SegmentViewMode[]) {
    segmentRanges[id] = { min: 0, max: 0 };
  }
  return {
    segmentRanges,
    temperature: { min: 0, max: 0 },
    layerTime: { min: 0, max: 0 },
    fans: [],
  };
}

/**
 * Scan the whole model once for every scalar channel's range — per-segment
 * (speed/flow/width/height) and per-layer (temperature/layer-time/fans) — so
 * switching the "Color by" mode never re-reads the WASM buffer.
 */
function scanModel(handle: GcodeHandle): ModelScan {
  const segIds = Object.keys(SCALAR_CHANNELS) as SegmentViewMode[];
  const segMin = {} as Record<SegmentViewMode, number>;
  const segMax = {} as Record<SegmentViewMode, number>;
  for (const id of segIds) {
    segMin[id] = Infinity;
    segMax[id] = 0;
  }

  let tempMin = Infinity;
  let tempMax = 0;
  let timeMin = Infinity;
  let timeMax = 0;

  const fanOrder: string[] = [];
  const fanMin = new Map<string, number>();
  const fanMax = new Map<string, number>();

  const layerCount = handle.layerCount();
  for (let li = 0; li < layerCount; li++) {
    const layer = handle.getLayer(li);

    // Per-segment scalars.
    const blockCount = layer.blocksCount();
    for (let b = 0; b < blockCount; b++) {
      const roleId = layer.blockRole(b);
      if (roleId === TRAVEL_ROLE_ID || roleId === SEAM_ROLE_ID) {
        continue;
      }
      const data = layer.blockData(b);
      for (let o = 0; o < data.length; o += FLOATS_PER_SEGMENT) {
        const w = data[o + WIDTH_OFFSET];
        const h = data[o + HEIGHT_OFFSET];
        const s = data[o + SPEED_OFFSET];
        if (s <= 0) {
          continue;
        }
        for (const id of segIds) {
          const v = SCALAR_CHANNELS[id].extract(w, h, s);
          if (v > 0) {
            if (v < segMin[id]) segMin[id] = v;
            if (v > segMax[id]) segMax[id] = v;
          }
        }
      }
    }

    // Per-layer scalars.
    const temp = layer.nozzleTemp();
    if (temp > 0) {
      if (temp < tempMin) tempMin = temp;
      if (temp > tempMax) tempMax = temp;
    }
    const t = layer.layerTimeS();
    if (t > 0) {
      if (t < timeMin) timeMin = t;
      if (t > timeMax) timeMax = t;
    }
    const fanCount = layer.fanCount();
    for (let f = 0; f < fanCount; f++) {
      const key = layer.fanKey(f);
      const speed = layer.fanSpeed(f);
      const curMin = fanMin.get(key);
      if (curMin === undefined) {
        fanOrder.push(key);
        fanMin.set(key, speed);
        fanMax.set(key, speed);
      } else {
        if (speed < curMin) fanMin.set(key, speed);
        if (speed > (fanMax.get(key) ?? speed)) fanMax.set(key, speed);
      }
    }
  }

  const segmentRanges = {} as Record<SegmentViewMode, ScalarRange>;
  for (const id of segIds) {
    segmentRanges[id] = Number.isFinite(segMin[id])
      ? { min: segMin[id], max: segMax[id] }
      : { min: 0, max: 0 };
  }

  const fans: FanInfo[] = fanOrder.map((key) => ({
    key,
    label: fanLabel(key),
    range: { min: fanMin.get(key) ?? 0, max: fanMax.get(key) ?? 0 },
  }));

  return {
    segmentRanges,
    temperature: Number.isFinite(tempMin) ? { min: tempMin, max: tempMax } : { min: 0, max: 0 },
    layerTime: Number.isFinite(timeMin) ? { min: timeMin, max: timeMax } : { min: 0, max: 0 },
    fans,
  };
}

/** Live hover readout for the G-code inspector tooltip and legend tick. */
export interface GcodeHoverInfo {
  channelId: GcodeViewMode;
  value: number;
  valueLabel: string;
  role: RoleName;
  layerIndex: number;
  z: number;
  width: number;
  height: number;
  speed: number;
  /** Normalized position on the active gradient, `[0, 1]`. */
  t: number;
  /** Viewport pointer position (px) used as the floating anchor. */
  clientX: number;
  clientY: number;
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

  /** Object-id set of the previously loaded slice, for scene-change detection. */
  #lastSlicedObjectIds: ReadonlySet<string> | null = null;

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
  readonly hiddenRoles = signal<ReadonlySet<RoleName>>(new Set(DEFAULT_HIDDEN_ROLES));

  /** Active coloring mode: role categories (default) or a scalar channel. */
  readonly viewMode = signal<GcodeViewMode>('category');

  /** Aggregated scalar metadata (ranges + discovered fans) of the loaded model. */
  readonly modelScan = signal<ModelScan>(emptyModelScan());

  /** Fans discovered in the loaded model, for the fan sub-selector. */
  readonly discoveredFans = computed<readonly FanInfo[]>(() => this.modelScan().fans);

  /** Selected fan key for the fan-speed view (first discovered fan by default). */
  readonly selectedFan = signal<string | null>(null);

  /** Live inspector readout for the extrusion under the cursor (or `null`). */
  readonly hoverInfo = signal<GcodeHoverInfo | null>(null);

  /**
   * Value band (in active-channel units) to spotlight while hovering the
   * legend; out-of-band extrusions dim so the matching ones stand out.
   */
  readonly hoverBand = signal<{ lo: number; hi: number } | null>(null);

  /**
   * View modes actually available for the loaded model: segment scalars are
   * always offered; fan/temperature/layer-time only when that data is present.
   */
  readonly availableViewModes = computed<readonly GcodeViewMode[]>(() => {
    const scan = this.modelScan();
    return VIEW_MODE_ORDER.filter((mode) => {
      if (mode === 'fan') return scan.fans.length > 0;
      if (mode === 'temperature') return scan.temperature.max > 0;
      if (mode === 'layerTime') return scan.layerTime.max > 0;
      return true;
    });
  });

  /**
   * The mode actually shown, derived from the user's choice clamped to what the
   * current model supports. This is the **single source of truth** every
   * consumer (dropdown, legend, 3D recolor) reads, so they can never disagree —
   * if a reslice drops the data behind the chosen mode it gracefully falls back
   * to `category` here rather than being imperatively reset elsewhere. The raw
   * `viewMode` is preserved so the mode auto-restores when the data returns.
   */
  readonly effectiveViewMode = computed<GcodeViewMode>(() => {
    const mode = this.viewMode();
    return this.availableViewModes().includes(mode) ? mode : 'category';
  });

  /** Range of the active scalar channel; `{0,0}` for the categorical mode. */
  readonly activeRange = computed<ScalarRange>(() => {
    const scan = this.modelScan();
    switch (this.effectiveViewMode()) {
      case 'category':
        return { min: 0, max: 0 };
      case 'fan': {
        const key = this.selectedFan();
        return scan.fans.find((f) => f.key === key)?.range ?? { min: 0, max: 0 };
      }
      case 'temperature':
        return scan.temperature;
      case 'layerTime':
        return scan.layerTime;
      default:
        return scan.segmentRanges[this.effectiveViewMode() as SegmentViewMode];
    }
  });

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
    // Entering fan mode with no selection yet: default to the first fan.
    if (mode === 'fan' && this.selectedFan() === null) {
      const first = this.modelScan().fans[0]?.key ?? null;
      this.selectedFan.set(first);
    }
    this.viewMode.set(mode);
  }

  setSelectedFan(key: string): void {
    this.selectedFan.set(key);
  }

  setHoverInfo(info: GcodeHoverInfo | null): void {
    this.hoverInfo.set(info);
  }

  setHoverBand(band: { lo: number; hi: number } | null): void {
    this.hoverBand.set(band);
  }

  /**
   * Discard the current preview so the layer/segment controls hide. Used when
   * the workplate is cleared — the slicer's `null` download URL is ignored by
   * the reload effect, so the handle must be dropped explicitly.
   */
  clear(): void {
    this.gcodeHandle.set(null);
    this.hoverInfo.set(null);
    this.hoverBand.set(null);
    this.modelScan.set(emptyModelScan());
    this.layerMax.set(0);
    this.segmentProgress.set(1);
    this.showAllLayers.set(true);
    this.#lastSlicedObjectIds = null;
  }

  // ── Private ──────────────────────────────────────────────────────────────

  /**
   * A reslice is the *same* scene when it shares at least one object id with
   * the previous slice. Moving, adding or removing objects keeps some ids (ids
   * are monotonic and never reused), so only a fully disjoint set — a brand-new
   * scene — returns `false`.
   */
  #isSameScene(current: ReadonlySet<string>): boolean {
    const prev = this.#lastSlicedObjectIds;
    if (!prev || prev.size === 0 || current.size === 0) {
      return false;
    }
    for (const id of current) {
      if (prev.has(id)) {
        return true;
      }
    }
    return false;
  }

  async #loadFromUrl(url: string): Promise<void> {
    this.loading.set(true);

    // Decide — without subscribing this reactive context to the signals read
    // here — whether this reslice is the same scene as the last, and snapshot
    // the outgoing layer position so it can be carried across the reslice.
    const carry = untracked(() => {
      const prevCount = this.layerCount();
      const prevMax = this.layerMax();
      const sceneIds = new Set(this.slicer.slicedObjectIds());
      const sameScene = this.#isSameScene(sceneIds);
      this.#lastSlicedObjectIds = sceneIds;
      return {
        sameScene,
        prevMax,
        wasAtTop: prevCount === 0 || prevMax >= prevCount - 1,
      };
    });

    this.gcodeHandle.set(null);
    this.hoverInfo.set(null);
    this.hoverBand.set(null);
    try {
      await init({ module_or_path: 'scene_engine_bg.wasm' });
      const response = await fetch(url);
      const buffer = await response.arrayBuffer();
      const handle = GcodeHandle.parse(new Uint8Array(buffer));
      const count = handle.layerCount();
      const scan = scanModel(handle);
      this.modelScan.set(scan);
      this.gcodeHandle.set(handle);

      if (carry.sameScene) {
        // Same scene resliced (objects moved / added / removed): keep the
        // user's current layer, progress, coloring mode and role toggles. If
        // they were viewing the whole model (top layer), stay pinned to the
        // new top even when the layer count changed.
        const layer = carry.wasAtTop ? count - 1 : Math.min(carry.prevMax, count - 1);
        this.layerMax.set(Math.max(0, layer));
      } else {
        // Brand-new scene: reset the viewer to its defaults. The coloring mode
        // is intentionally *not* reset here — `effectiveViewMode` clamps it to
        // what the model supports, so it stays consistent (and sticky) without
        // an imperative reset that would race the reactive rebuild.
        this.layerMax.set(Math.max(0, count - 1));
        this.segmentProgress.set(1);
        this.hiddenRoles.set(new Set(DEFAULT_HIDDEN_ROLES));
        this.showAllLayers.set(true);
      }

      // Re-validate the fan selection against the new scan (a reslice may drop
      // the previously selected fan). The active *mode* needs no re-validation:
      // `effectiveViewMode` derives it from `availableViewModes`.
      const fan = this.selectedFan();
      if (fan === null || !scan.fans.some((f) => f.key === fan)) {
        this.selectedFan.set(scan.fans[0]?.key ?? null);
      }
    } catch (error) {
      console.error('[GcodePreview] Failed to load gcode:', error);
    } finally {
      this.loading.set(false);
    }
  }
}
