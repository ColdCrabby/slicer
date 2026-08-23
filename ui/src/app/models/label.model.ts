import { uid } from './id';

/**
 * A user-defined, cross-area label. Labels are freely created by the user and
 * assigned to any profile (printer / filament / print profile) to organise and
 * filter large collections — think macOS Finder tags with GitHub-style colours.
 *
 * A label owns an id, a name, a base hue ({@link Label.color}), and a
 * {@link LabelTone}; the two together resolve to the subtle tint used
 * everywhere the label is shown. *What* it is attached to lives on each profile
 * as {@link ProfileMeta.labelIds} — labels stay a single flat, shared
 * vocabulary rather than three parallel per-area lists.
 */
export interface Label {
  id: string;
  name: string;
  /** Base hue — one of {@link LABEL_HUES}' values (a muted hex). */
  color: string;
  /** Shade: `dark` is a deeper tint, `light` is softer / more transparent. */
  tone: LabelTone;
}

/**
 * Two shades per hue, mirroring GitHub's light/dark label variants:
 * - `dark`  — deeper, slightly stronger tint.
 * - `light` — the *same hue* rendered more transparent, so it reads softer.
 *
 * Neither is fully saturated — every label stays subtle against the app's own
 * chrome (see the tint recipe in `label-chip`).
 */
export type LabelTone = 'dark' | 'light';
export const LABEL_TONES: LabelTone[] = ['dark', 'light'];

/** One selectable hue in the picker. */
export interface LabelHue {
  name: string;
  value: string;
}

/**
 * A deliberately small set of muted base hues covering the spectrum. Each is
 * already desaturated so that, once tinted by the chip recipe, no label ever
 * competes with the amber accent or the app surfaces.
 */
export const LABEL_HUES: readonly LabelHue[] = [
  { name: 'Gray', value: '#6b7280' },
  { name: 'Red', value: '#c05a56' },
  { name: 'Orange', value: '#c2793f' },
  { name: 'Amber', value: '#b8942f' },
  { name: 'Green', value: '#5a9a5e' },
  { name: 'Teal', value: '#3f9391' },
  { name: 'Blue', value: '#5b82c4' },
  { name: 'Purple', value: '#8a6fbf' },
];

/** Pick a random hue value (used when creating a label inline). */
export function randomLabelHue(): string {
  return LABEL_HUES[Math.floor(Math.random() * LABEL_HUES.length)].value;
}

/**
 * A `color-mix` string for a small label dot (dropdown swatches, picker rows).
 * `light`-toned labels render more transparent so the dot echoes the chip's
 * softness. Usable directly as a CSS `background` value.
 */
export function labelDotColor(label: Label): string {
  const pct = label.tone === 'light' ? 55 : 88;
  return `color-mix(in oklab, ${label.color} ${pct}%, transparent)`;
}

export function makeLabel(overrides: Partial<Label> = {}): Label {
  return {
    id: uid(),
    name: 'New label',
    color: randomLabelHue(),
    tone: 'dark',
    ...overrides,
  };
}

/** Seed labels so the feature is immediately discoverable on first run. */
export const DEFAULT_LABELS: Label[] = [
  { id: 'label-favorite', name: 'Favorite', color: '#b8942f', tone: 'dark' },
  { id: 'label-calibrated', name: 'Calibrated', color: '#5a9a5e', tone: 'dark' },
  { id: 'label-experimental', name: 'Experimental', color: '#8a6fbf', tone: 'light' },
];
