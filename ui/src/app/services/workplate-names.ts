import { Injectable, inject } from '@angular/core';
import { BrowserStorage } from './browser-storage';
import { WorkplateSettingsStore } from './workplate-settings';

/** Where names used to live, before they became part of the plate's document. */
const LEGACY_STORAGE_KEY = 'workplate.names';
const DEFAULT_WORKPLATE_NAME = 'Untitled workplate';
const DEFAULT_GCODE_FILENAME = 'output.gcode';
const INVALID_FILENAME_CHARS = /[<>:"/\\|?*\u0000-\u001F]/g;
const GCODE_EXTENSION = /\.(gcode|gco|g)$/i;

/**
 * What each workplate is called, and everything derived from it — the tab
 * label, the scene title, the history row, the G-code filename.
 *
 * The name itself is **not stored here.** It is a field of the plate's own
 * document ({@link WorkplateSettingsStore}), so a tab renamed on the desktop is
 * still that tab when the plate is opened on an iPad, and clearing a browser
 * does not take every name with it. What lives here is the rest of the answer:
 * which fallback to use when a plate was never renamed, and how to turn a name
 * into something a filesystem will accept.
 */
@Injectable({ providedIn: 'root' })
export class WorkplateNames {
  private readonly storage = inject(BrowserStorage);
  private readonly plates = inject(WorkplateSettingsStore);

  constructor() {
    this.#migrateLegacyNames();
  }

  /** The custom name for a workplate, or `null` if it was never renamed. */
  nameFor(uuid: string | null | undefined): string | null {
    return uuid ? (this.plates.settingsFor(uuid).name ?? null) : null;
  }

  /**
   * The human-facing workplate title:
   * custom rename → uploaded model stem → fallback.
   */
  displayNameFor(
    uuid: string | null | undefined,
    sourceFilename: string | null | undefined,
  ): string {
    return (
      this.nameFor(uuid) ?? this.defaultNameFromFilename(sourceFilename) ?? DEFAULT_WORKPLATE_NAME
    );
  }

  /** Derive the default plate name from an uploaded model filename. */
  defaultNameFromFilename(filename: string | null | undefined): string | null {
    if (!filename) {
      return null;
    }

    const basename = filename.trim().split(/[\\/]/).pop()?.trim();
    if (!basename) {
      return null;
    }

    const stem = basename.replace(/\.[^./\\]+$/, '').trim();
    return stem || null;
  }

  /**
   * Canonical `<workplate>.gcode` filename used for downloads and printer sends.
   */
  gcodeFilenameFor(
    uuid: string | null | undefined,
    sourceFilename: string | null | undefined,
  ): string {
    const baseName = this.nameFor(uuid) ?? this.defaultNameFromFilename(sourceFilename);
    if (!baseName) {
      return DEFAULT_GCODE_FILENAME;
    }

    const safeBase = this.#sanitizeFilenameBase(baseName);
    if (!safeBase) {
      return DEFAULT_GCODE_FILENAME;
    }

    const withoutGcodeExt = safeBase.replace(GCODE_EXTENSION, '').trim();
    return withoutGcodeExt ? `${withoutGcodeExt}.gcode` : DEFAULT_GCODE_FILENAME;
  }

  /**
   * Canonical `<workplate>.3mf` filename used when exporting the plate.
   * Falls back to the plate's display name, so an unnamed plate still exports
   * as something recognisable.
   */
  threeMfFilenameFor(
    uuid: string | null | undefined,
    sourceFilename: string | null | undefined,
  ): string {
    const safeBase = this.#sanitizeFilenameBase(this.displayNameFor(uuid, sourceFilename));
    return `${safeBase || DEFAULT_WORKPLATE_NAME}.3mf`;
  }

  /** Store (or, when blank, clear) the custom name for a workplate. */
  setName(uuid: string, name: string): void {
    this.plates.setName(uuid, name.trim() || null);
  }

  /**
   * Fold names written before they were part of the plate's document into it,
   * once, then drop the old map so this cannot run twice.
   *
   * A build that has names in the old place and nothing in the new one would
   * otherwise look, to the user, exactly like every plate they ever renamed
   * having forgotten its name.
   */
  #migrateLegacyNames(): void {
    const legacy = this.storage.getJson<Record<string, string>>(LEGACY_STORAGE_KEY, 'local');
    if (!legacy) {
      return;
    }
    for (const [uuid, name] of Object.entries(legacy)) {
      if (name && !this.plates.settingsFor(uuid).name) {
        this.plates.setName(uuid, name);
      }
    }
    this.storage.write(LEGACY_STORAGE_KEY, null, 'local');
  }

  #sanitizeFilenameBase(name: string): string {
    return name
      .replace(INVALID_FILENAME_CHARS, ' ')
      .replace(/\s+/g, ' ')
      .replace(/\.+$/, '')
      .trim();
  }
}
