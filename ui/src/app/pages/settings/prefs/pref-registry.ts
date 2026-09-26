/**
 * Every app preference in Settings, by id: its name, the one line that sits
 * under it, the longer explanation behind its ⓘ, and the words someone might
 * search for it by.
 *
 * **One copy.** A preference row renders its title and text from here
 * (`nexus-pref-row`), and the Settings search indexes the same entries, so a
 * row cannot exist without being findable and the search can never name a
 * setting by words the page no longer uses.
 *
 * The line under the title is the glance — what the control is for, in
 * roughly half a sentence. Anything that takes a second sentence (why Auto
 * decides what it decides, what a choice costs, what else it drags with it)
 * belongs in `detail`, which the row reveals on demand. That split is the whole
 * difference between a page that can be scanned and one that has to be read.
 */

/** A page of app preferences, by its route under `/settings`. */
export type PrefPage = 'general' | 'appearance' | '3d-view' | 'controls';

export interface PrefDef {
  /** Stable id; the row's anchor is `pref-<id>`. */
  id: string;
  page: PrefPage;
  /** The group heading it sits under on its page. */
  group: string;
  title: string;
  /** The line under the title. A page may replace it with a live one. */
  hint?: string;
  /** What the ⓘ reveals. */
  detail?: string;
  /** Other words for it — including what other apps call it — for search only. */
  keywords?: string;
}

export const PREFS: readonly PrefDef[] = [
  // ── General ────────────────────────────────────────────────────────────
  {
    id: 'auto-slice',
    page: 'general',
    group: 'Slicing',
    title: 'Re-slice after a change',
    hint: 'Slice again on its own when the plate or a setting changes.',
    detail:
      'Automatic only re-slices while the open plate slices quickly, and leaves a slow plate to the Slice button. It starts out re-slicing and settles once it has timed a slice.',
    keywords: 'auto slice automatic reslice live update background',
  },
  {
    id: 'preview-follow',
    page: 'general',
    group: 'Slicing',
    title: 'Show the preview after slicing',
    hint: 'Switch to the G-code view when a slice finishes.',
    detail: 'Automatic switches for slices you start, and stays put for automatic ones.',
    keywords: 'gcode view switch toolpath preview jump',
  },
  {
    id: 'settings-detail',
    page: 'general',
    group: 'Settings panels',
    title: 'Settings detail',
    hint: 'How much each settings panel shows before you ask for more.',
    detail:
      'Every setting stays reachable from search and from the controls in each section — this only moves where the panels start.',
    keywords: 'tier advanced expert everything standard simple more options',
  },
  {
    id: 'library-export',
    page: 'general',
    group: 'Your library',
    title: 'Export profile library',
    hint: 'Printers, filaments, processes and labels as TOML.',
    detail:
      'The same format the slicer reads. Export a bundle of separate files, or one profiles.toml for the command line. Printer API keys are left out, so the file is safe to share.',
    keywords: 'backup toml download profiles.toml save copy share',
  },
  {
    id: 'library-storage',
    page: 'general',
    group: 'Your library',
    title: 'Where your library lives',
    keywords: 'storage saved browser device server cloud local data sync',
  },
  {
    id: 'about-version',
    page: 'general',
    group: 'About',
    title: 'Version',
    keywords: 'about build release commit update changelog',
  },

  // ── Appearance ─────────────────────────────────────────────────────────
  {
    id: 'theme',
    page: 'appearance',
    group: 'Theme',
    title: 'Theme',
    hint: 'Light, dark, or follow the system.',
    keywords: 'dark mode light mode night appearance system',
  },
  {
    id: 'accent',
    page: 'appearance',
    group: 'Accent colour',
    title: 'Accent colour',
    hint: 'The colour of selections, focus and primary buttons.',
    keywords: 'accent color highlight tint brand system colour',
  },
  {
    id: 'color-picker',
    page: 'appearance',
    group: 'Colour picker',
    title: 'Colour picker',
    hint: 'Which picker opens when you choose a colour.',
    keywords: 'color picker native os dialog swatch',
  },

  // ── 3D View ────────────────────────────────────────────────────────────
  {
    id: 'filament-color',
    page: '3d-view',
    group: 'Look',
    title: 'Colour models by filament',
    hint: 'Tint models with the active filament instead of a neutral tone.',
    keywords: 'filament colour color model mesh tint',
  },
  {
    id: 'model-shading',
    page: '3d-view',
    group: 'Look',
    title: 'Shading',
    hint: 'Smooth across curves, or every facet visible.',
    detail: 'Flat shows each triangle distinctly, for a low-poly CAD look.',
    keywords: 'smooth flat facets normals low poly cad shading',
  },
  {
    id: 'gloss',
    page: '3d-view',
    group: 'Look',
    title: 'Gloss',
    hint: 'A specular sheen on models and the G-code preview.',
    detail: 'Off gives a flat matte look.',
    keywords: 'specular shiny wet filament matte highlight',
  },
  {
    id: 'shadows',
    page: '3d-view',
    group: 'Look',
    title: 'Shadows',
    hint: 'A soft shadow from models onto the build plate.',
    detail: 'Grounds the scene and adds contrast. Turn it off on weaker GPUs.',
    keywords: 'shadow contact ground ambient occlusion gpu',
  },
  {
    id: 'fov',
    page: '3d-view',
    group: 'Camera',
    title: 'Field of view',
    hint: 'Lower is flatter and closer; higher shows more of the scene.',
    keywords: 'fov perspective camera angle zoom lens',
  },
  {
    id: 'antialiasing',
    page: '3d-view',
    group: 'Performance',
    title: 'Anti-aliasing',
    hint: 'Smooths jagged edges.',
    detail:
      'Auto turns it off on high-density displays, where it barely helps. Changing this reloads the 3D view.',
    keywords: 'antialias msaa aa jaggies smooth edges',
  },
  {
    id: 'render-quality',
    page: '3d-view',
    group: 'Performance',
    title: 'Render resolution',
    hint: 'Sharper costs GPU; lower favours frame rate.',
    keywords: 'resolution pixel ratio dpi retina sharpness fps gpu',
  },
  {
    id: 'preview-detail',
    page: '3d-view',
    group: 'Performance',
    title: 'Preview detail',
    hint: 'How finely sliced extrusions are drawn.',
    detail:
      'Auto keeps the full, rounded bead whenever it runs smoothly — including every time you stop moving — and only simplifies while you orbit a heavy plate.',
    keywords: 'gcode toolpath bead extrusion lod detail quality performance',
  },
  {
    id: 'thumbnail-look',
    page: '3d-view',
    group: 'Thumbnails',
    title: 'Thumbnail look',
    hint: 'Plain previews identically everywhere; Match borrows this view.',
    detail:
      'The preview embedded in sliced G-code is shot here, in the 3D view. Plain renders it flatly, so the same plate previews identically on every machine; Match this view borrows the shading, gloss and contact shadow above. Its angle, background and colour are print settings, under Processes → Thumbnail.',
    keywords: 'thumbnail preview image gcode embedded screenshot printer screen',
  },
  {
    id: 'thumbnail-fx',
    page: '3d-view',
    group: 'Thumbnails',
    title: 'Capture animation',
    hint: 'Flash the view when a slice takes its thumbnail.',
    detail: 'The captured preview flies off to the top. Turn it off to shoot silently.',
    keywords: 'screenshot flash animation capture thumbnail effect',
  },
  {
    id: 'stats',
    page: '3d-view',
    group: 'Diagnostics',
    title: 'Performance chips',
    hint: 'FPS and WASM timings over the 3D view.',
    keywords: 'fps frame rate stats diagnostics wasm timing debug overlay',
  },

  // ── Controls ───────────────────────────────────────────────────────────
  {
    id: 'two-finger',
    page: 'controls',
    group: 'Trackpad & touch',
    title: 'Two-finger swipe',
    hint: 'What a trackpad swipe does in the 3D view.',
    keywords: 'trackpad gesture orbit pan scroll swipe navigation',
  },
  {
    id: 'palm-rejection',
    page: 'controls',
    group: 'Trackpad & touch',
    title: 'Palm rejection',
    hint: 'Ignore a hand resting on the screen while using a stylus.',
    detail:
      'While drawing with an Apple Pencil or stylus, the palm never orbits or zooms the 3D view. Finger-only gestures are unaffected.',
    keywords: 'apple pencil stylus pen palm touch ipad tablet',
  },
  {
    id: 'history-buttons',
    page: 'controls',
    group: 'On-screen buttons',
    title: 'Undo and redo buttons',
    hint: 'Back and forward buttons in the 3D view toolbar.',
    detail:
      'Auto shows them only on touch devices without a keyboard, where the ⌘/Ctrl+Z shortcut is unavailable.',
    keywords: 'undo redo history toolbar buttons touch',
  },
  {
    id: 'gcode-steps',
    page: 'controls',
    group: 'On-screen buttons',
    title: 'G-code step buttons',
    hint: 'Step the layer and progress sliders one at a time.',
    detail:
      'The same as the arrow keys. Auto shows them wherever there is no keyboard to press those with — every touchscreen — and hides them on a mouse or trackpad.',
    keywords: 'gcode layer slider step buttons arrows touch',
  },
  {
    id: 'shortcuts',
    page: 'controls',
    group: 'Keyboard shortcuts',
    title: 'Keyboard shortcuts',
    keywords: 'hotkeys keys keyboard shortcut bindings',
  },
];

const BY_ID = new Map(PREFS.map((pref) => [pref.id, pref]));

/** The preference with `id`. Throws on an unknown id — a typo is a bug, not a blank row. */
export function prefById(id: string): PrefDef {
  const pref = BY_ID.get(id);
  if (!pref) {
    throw new Error(`Unknown preference "${id}" — add it to PREFS in pref-registry.ts`);
  }
  return pref;
}

/** The anchor a preference's row carries, and a search result scrolls to. */
export function prefAnchor(id: string): string {
  return `pref-${id}`;
}
