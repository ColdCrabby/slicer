import { Injectable, effect, inject, signal } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { NavigationEnd, Router } from '@angular/router';
import { filter, map, startWith } from 'rxjs';
import { BrowserStorage } from './browser-storage';
import { SlicerFile } from './slicer-file';

const STORAGE_KEY = 'workplate.open-tabs';

/** One open tab: a workplate's `request_uuid` plus the filename it was last seen with. */
export interface OpenWorkplateTab {
  uuid: string;
  filename: string | null;
}

/**
 * Tracks which workplates are open as tabs in the titlebar.
 *
 * Server scenes are ephemeral per WS connection (see AGENTS.md's Scene Engine
 * section) — only one plate is ever actually loaded at a time. This service
 * does not change that; it is purely the UI-level "which tabs are open" list,
 * the same way a browser keeps several tabs open while only one page is
 * frontmost. Switching tabs still goes through the existing `/slice/:requestUuid`
 * route, which already reloads that plate's scene on navigation.
 */
@Injectable({ providedIn: 'root' })
export class OpenWorkplates {
  private readonly storage = inject(BrowserStorage);
  private readonly router = inject(Router);
  private readonly slicerFile = inject(SlicerFile);

  private readonly _tabs = signal<readonly OpenWorkplateTab[]>(
    this.storage.getJson<OpenWorkplateTab[]>(STORAGE_KEY, 'local') ?? [],
  );

  /** Open workplate tabs, in the order they were opened. */
  readonly tabs = this._tabs.asReadonly();

  /** The `:requestUuid` of the route currently on screen, or `null` off `/slice`. */
  readonly activeUuid = toSignal(
    this.router.events.pipe(
      filter((event) => event instanceof NavigationEnd),
      startWith(null),
      map(() => this.#uuidFromUrl(this.router.url)),
    ),
    { initialValue: this.#uuidFromUrl(this.router.url) },
  );

  constructor() {
    // A plate becomes a tab the moment it's loaded — upload, history entry, or
    // deep link — mirroring how a browser tab opens the moment its page loads,
    // not only when the user explicitly asks for a new one.
    effect(() => {
      const uuid = this.slicerFile.requestUuid();
      const filename = this.slicerFile.sourceFilename();
      if (uuid) {
        this.open(uuid, filename);
      }
    });
  }

  /** Add (or update the remembered filename of) a tab. */
  open(uuid: string, filename?: string | null): void {
    const tabs = this._tabs();
    const existing = tabs.find((tab) => tab.uuid === uuid);
    if (existing) {
      if (filename && !existing.filename) {
        this.#persist(tabs.map((tab) => (tab.uuid === uuid ? { ...tab, filename } : tab)));
      }
      return;
    }
    this.#persist([...tabs, { uuid, filename: filename ?? null }]);
  }

  /**
   * Close a tab. If it was the one on screen, navigates to its neighbor —
   * or to a fresh plate when it was the last tab open.
   */
  close(uuid: string): void {
    const tabs = this._tabs();
    const index = tabs.findIndex((tab) => tab.uuid === uuid);
    if (index === -1) {
      return;
    }
    const remaining = tabs.filter((tab) => tab.uuid !== uuid);
    this.#persist(remaining);

    if (this.activeUuid() !== uuid) {
      return;
    }
    const fallback = remaining[index] ?? remaining[index - 1];
    this.router.navigate(['/slice', fallback?.uuid ?? 'new']);
  }

  #persist(tabs: readonly OpenWorkplateTab[]): void {
    this._tabs.set(tabs);
    this.storage.writeJson(STORAGE_KEY, tabs, 'local');
  }

  /** Extract the workplate UUID from a `/slice/:requestUuid` URL. */
  #uuidFromUrl(url: string): string | null {
    const match = url.split(/[?#]/)[0].match(/^\/slice\/([^/]+)$/);
    if (!match) {
      return null;
    }
    const segment = decodeURIComponent(match[1]);
    return segment === 'new' ? null : segment;
  }
}
