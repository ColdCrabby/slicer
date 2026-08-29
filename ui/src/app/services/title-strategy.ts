import { inject, Injectable } from '@angular/core';
import { TitleStrategy } from '@angular/router';
import type { RouterStateSnapshot } from '@angular/router';
import { Title } from '@angular/platform-browser';

const APP_NAME = 'Cold Crabby';

/**
 * Sets the browser tab title from each route's `title`, suffixed with the app
 * name (e.g. "Settings — Cold Crabby"). Routes without a title fall back to
 * the bare app name.
 */
@Injectable({ providedIn: 'root' })
export class NexusTitleStrategy extends TitleStrategy {
  private readonly title = inject(Title);

  override updateTitle(snapshot: RouterStateSnapshot): void {
    const page = this.buildTitle(snapshot);
    this.title.setTitle(page ? `${page} · ${APP_NAME}` : APP_NAME);
  }
}
