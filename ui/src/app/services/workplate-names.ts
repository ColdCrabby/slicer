import { Injectable, inject, signal } from '@angular/core';
import { BrowserStorage } from './browser-storage';

const STORAGE_KEY = 'workplate.names';
const DEFAULT_WORKPLATE_NAME = 'Untitled workplate';
const DEFAULT_GCODE_FILENAME = 'output.gcode';
const INVALID_FILENAME_CHARS = /[<>:"/\\|?*\u0000-\u001F]/g;
const GCODE_EXTENSION = /\.(gcode|gco|g)$/i;

/**
 * Remembers the user-chosen display name for each workplate, keyed by its
 * `request_uuid`. Backend scenes are ephemeral per WS connection, so — like the
 * printer and filament profiles — names live in localStorage and survive
 * reloads. Editing a name here is the single source of truth the scene title
 * and history list both read from.
 */
@Injectable({ providedIn: 'root' })
export class WorkplateNames {
  private readonly storage = inject(BrowserStorage);

  private readonly _names = signal<Record<string, string>>(
    this.storage.getJson<Record<string, string>>(STORAGE_KEY, 'local') ?? {},
  );

  /** Reactive map of `request_uuid` → custom name. */
  readonly names = this._names.asReadonly();

  /** The custom name for a workplate, or `null` if it was never renamed. */
  nameFor(uuid: string | null | undefined): string | null {
    if (!uuid) {
      return null;
    }
    return this._names()[uuid] ?? null;
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

  /** Store (or, when blank, clear) the custom name for a workplate. */
  setName(uuid: string, name: string): void {
    const trimmed = name.trim();
    this._names.update((map) => {
      const next = { ...map };
      if (trimmed) {
        next[uuid] = trimmed;
      } else {
        delete next[uuid];
      }
      return next;
    });
    this.storage.writeJson(STORAGE_KEY, this._names(), 'local');
  }

  #sanitizeFilenameBase(name: string): string {
    return name
      .replace(INVALID_FILENAME_CHARS, ' ')
      .replace(/\s+/g, ' ')
      .replace(/\.+$/, '')
      .trim();
  }
}
