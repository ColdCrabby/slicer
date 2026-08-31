import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { Viewport } from '../../services/viewport';
import { NavRail } from '../nav-rail/nav-rail';
import { NexusTitlebar } from '../titlebar/titlebar';

/**
 * Global application shell: the custom title bar on top, a primary navigation
 * rail on the left, and the routed surface (dashboard / slice workspace /
 * settings) filling the rest. Wraps every route.
 *
 * On phones the rail becomes a bottom tab bar — see `shell.scss` and
 * `nav-rail.scss`; the flip is pure CSS, so it survives a cold start with no
 * script.
 */
@Component({
  selector: 'nexus-shell',
  imports: [RouterOutlet, NexusTitlebar, NavRail],
  templateUrl: './shell.html',
  styleUrl: './shell.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AppShell {
  /**
   * Constructed here purely so it exists for the whole session.
   *
   * `Viewport` keeps the `is-handheld` class on `<html>` in step with the
   * viewport, and that class is what the phone overrides for the shared
   * `@coldcrabby/ui` components hang off. Left to lazy injection it would only
   * come alive on routes that happen to ask a component for it, so rotating the
   * device on the dashboard would leave the class stale.
   */
  private readonly viewport = inject(Viewport);
}
