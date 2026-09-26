import { Injectable, inject } from '@angular/core';
import { Router } from '@angular/router';
import { listen } from '@tauri-apps/api/event';
import { openExternal } from './external-links';
import { Logger } from './logger';
import { Slicer } from './slicer';
import { ViewerControl } from './viewer-control';

/** Event the macOS menu bar emits an app command on (`app_menu.rs`); workplate
 * commands arrive on their own event. */
const MENU_EVENT = 'app-menu';

const DOCS_URL = 'https://slicer.maxscopp.de/docs/';

/**
 * Runs what the native menu bar asks for.
 *
 * The menu is built in Rust and owns no behaviour: each item emits its id, and
 * this maps the id onto the code the in-app controls already run, so a menu
 * command and the button beside the plate can never do different things.
 */
@Injectable({ providedIn: 'root' })
export class AppMenu {
  readonly #router = inject(Router);
  readonly #slicer = inject(Slicer);
  readonly #viewerControl = inject(ViewerControl);
  readonly #log = inject(Logger).scope('AppMenu');
  #started = false;

  /** Begin listening. Idempotent. */
  start(): void {
    if (this.#started) {
      return;
    }
    this.#started = true;
    listen<string>(MENU_EVENT, (event) => this.#run(event.payload)).catch((error: unknown) =>
      this.#log.error('could not subscribe to the menu bar', String(error)),
    );
  }

  #run(id: string): void {
    switch (id) {
      case 'settings':
        void this.#router.navigate(['/settings']);
        return;
      case 'add-model':
        // On a plate, the toolbar's own picker adds to it; anywhere else there
        // is no plate to add to, so start one.
        if (document.querySelector('.viewer-host')) {
          this.#viewerControl.addModelRequests.update((n) => n + 1);
        } else {
          void this.#router.navigate(['/slice/new']);
        }
        return;
      case 'slice':
        void this.#slicer.slice();
        return;
      case 'export-gcode':
        this.#slicer.downloadGcode();
        return;
      case 'help-docs':
        openExternal(DOCS_URL);
        return;
      case 'help-shortcuts':
        void this.#router.navigate(['/settings/controls'], { fragment: 'pref-shortcuts' });
        return;
    }
  }
}
