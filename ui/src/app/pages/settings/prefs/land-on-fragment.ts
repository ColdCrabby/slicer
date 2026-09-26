import { afterNextRender, inject } from '@angular/core';
import { ActivatedRoute } from '@angular/router';
import { focusConfigureTarget } from '../configure-scroll';

/**
 * Scroll to, and briefly mark, the element the URL's fragment names — once the
 * page has rendered. For links that land on one row of a preferences page: the
 * app menu's "Keyboard Shortcuts", and a Settings search result opened in a
 * new page.
 *
 * Call from a page's constructor. The Settings content column is its own
 * scroller, which the router's anchor scrolling does not know about, so the
 * page does it.
 */
export function landOnFragment(): void {
  const fragment = inject(ActivatedRoute).snapshot.fragment;
  if (fragment) {
    afterNextRender(() => focusConfigureTarget(`#${CSS.escape(fragment)}`));
  }
}
