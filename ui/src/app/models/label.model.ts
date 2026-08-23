import { uid } from './id';

/**
 * A user-defined, cross-area label. Labels are freely created by the user and
 * assigned to any profile (printer / filament / print profile) to organise and
 * filter large collections — think macOS Finder tags with GitHub-style colours.
 *
 * A label owns only an id, a name, and a colour; *what* it is attached to lives
 * on each profile as {@link ProfileMeta.labelIds}. That keeps labels a single
 * flat, shared vocabulary rather than three parallel per-area lists.
 */
export interface Label {
  id: string;
  name: string;
  /** Hex colour (e.g. `#e0730f`). Rendered as the chip/dot fill. */
  color: string;
}

/**
 * Curated default palette, mirroring the spread GitHub offers in its label
 * colour picker: saturated, evenly-spaced hues that read on both light and dark
 * surfaces. Users can still enter any custom hex.
 */
export const LABEL_PALETTE: readonly string[] = [
  '#e0730f', // amber (brand)
  '#d73a49', // red
  '#e36209', // orange
  '#dbab09', // yellow
  '#28a745', // green
  '#2188ff', // blue
  '#6f42c1', // purple
  '#ea4aaa', // pink
  '#0e8a16', // emerald
  '#006b75', // teal
  '#5319e7', // indigo
  '#b60205', // crimson
  '#fbca04', // gold
  '#0052cc', // cobalt
  '#5a6772', // slate
];

/** Pick a random colour from the palette (used by the "shuffle" affordance). */
export function randomLabelColor(): string {
  return LABEL_PALETTE[Math.floor(Math.random() * LABEL_PALETTE.length)];
}

export function makeLabel(overrides: Partial<Label> = {}): Label {
  return {
    id: uid(),
    name: 'New label',
    color: randomLabelColor(),
    ...overrides,
  };
}

/**
 * Readable text colour (black/white) for a given label background, chosen by
 * perceived luminance so chip text stays legible on any hue — the same trick
 * GitHub uses for its label text.
 */
export function labelTextColor(hex: string): string {
  const c = hex.replace('#', '');
  if (c.length < 6) {
    return '#ffffff';
  }
  const r = parseInt(c.slice(0, 2), 16);
  const g = parseInt(c.slice(2, 4), 16);
  const b = parseInt(c.slice(4, 6), 16);
  // Relative luminance (sRGB) — threshold tuned to match GitHub's contrast.
  const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  return luminance > 0.6 ? '#1b1b1f' : '#ffffff';
}

/** Whether a string is a syntactically valid 6-digit hex colour. */
export function isValidHexColor(value: string): boolean {
  return /^#[0-9a-fA-F]{6}$/.test(value.trim());
}

/** Seed labels so the feature is immediately discoverable on first run. */
export const DEFAULT_LABELS: Label[] = [
  { id: 'label-favorite', name: 'Favorite', color: '#dbab09' },
  { id: 'label-calibrated', name: 'Calibrated', color: '#28a745' },
  { id: 'label-experimental', name: 'Experimental', color: '#6f42c1' },
];
