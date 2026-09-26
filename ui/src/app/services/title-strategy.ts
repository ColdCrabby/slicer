import { inject, Injectable } from '@angular/core';
import { TitleStrategy } from '@angular/router';
import type { RouterStateSnapshot } from '@angular/router';
import { Title } from '@angular/platform-browser';

const APP_NAME = 'Cold Crabby';

/**
 * Sets the browser tab title from each route's `title`, suffixed with the app
 * name (e.g. "Settings · Cold Crabby"). Routes without a title — the home
 * screen — keep the title the page was served with: the bare app name, or the
 * descriptive one the public site's build writes for search engines
 * (scripts/seo/), which a crawler rendering the app must still find.
 */
@Injectable({ providedIn: 'root' })
export class NexusTitleStrategy extends TitleStrategy {
  private readonly title = inject(Title);
  /** Read before any route has had a chance to change it. */
  private readonly servedTitle = this.title.getTitle() || APP_NAME;

  override updateTitle(snapshot: RouterStateSnapshot): void {
    const page = this.buildTitle(snapshot);
    this.title.setTitle(page ? `${page} · ${APP_NAME}` : this.servedTitle);
  }
}
