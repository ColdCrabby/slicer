/**
 * Where a profile came from. Drives badges, edit/delete affordances, and the
 * "based on" lineage shown in the settings UI.
 *
 * - `builtin` — the single offline default that ships with the app for a given
 *   category. Always present, can be edited but not deleted (the store keeps at
 *   least one entry). Users are never blocked from creating their own.
 * - `user`    — created from scratch, duplicated, or imported+customised. Fully
 *   editable and removable.
 * - `catalog` — a read-only entry that lives in the cloud catalog. Catalog
 *   entries are never stored locally as-is; importing one produces a `user`
 *   copy whose {@link ProfileMeta.basedOn} points back at the catalog id.
 */
export type ProfileSource = 'builtin' | 'user' | 'catalog';

/**
 * Provenance fields shared by every profile kind. Matches the engine's
 * generated profile shape (snake_case, optional `source`), so the profile
 * models can be the engine types directly with no mapping.
 */
export interface ProfileMeta {
  id: string;
  name: string;
  source?: ProfileSource;
  /** Catalog id this profile was imported/derived from, if any. */
  based_on?: string | null;
  /**
   * Provenance for a profile fetched from the cloud catalog: the source API URL
   * it was imported from. Hidden field — never edited in the UI and ignored by
   * the slicer; an imported profile behaves exactly like a hand-made one.
   */
  import_url?: string | null;
  /**
   * Ids of the user-defined {@link Label}s attached to this profile. A single
   * flat, cross-area vocabulary (see `label.model.ts`) — the same label can be
   * attached to a printer, a filament, and a print profile.
   */
  label_ids?: string[];
  [k: string]: unknown;
}

export const PROFILE_SOURCE_LABELS: Record<ProfileSource, string> = {
  builtin: 'Built-in',
  user: 'Custom',
  catalog: 'Catalog',
};
