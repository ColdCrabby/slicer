import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { RouteProgress } from '../../components/route-progress/route-progress';
import { OmniboxService } from '../../omnibox/omnibox-service';
import { OmniboxPalette } from '../../omnibox/omnibox';
import { Viewport } from '../../services/viewport';
import { NavRail } from '../nav-rail/nav-rail';
import { NexusTitlebar } from '../titlebar/titlebar';

/**
 * Global application shell: the custom title bar on top, a primary navigation
 * rail on the left, and the routed surface (dashboard / slice workspace /
 * settings) filling the rest. Wraps every route.
 *
 * The surface itself is always lazily loaded, so the shell also carries the
 * {@link RouteProgress} hairline — the one place that spans every destination
 * and can therefore report the wait wherever the user is heading.
 *
 * On phones the rail becomes a bottom tab bar — see `shell.scss` and
 * `nav-rail.scss`; the flip is pure CSS, so it survives a cold start with no
 * script.
 */
@Component({
  selector: 'nexus-shell',
  // The omnibox palette is in the template behind `@defer (when omnibox.open())`
  // — listed here for the compiler, which is what keeps it out of the eager
  // bundle and in its own chunk until the palette is first asked for.
  imports: [RouterOutlet, NexusTitlebar, NavRail, RouteProgress, OmniboxPalette],
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

  /**
   * Whether the omnibox palette should be on screen.
   *
   * The shell never renders the palette itself — the template's `@defer` does,
   * when this turns true — but the trigger has to live somewhere eager, because
   * the keyboard shortcuts must reach the palette from any route before its
   * chunk has ever loaded.
   */
  protected readonly omnibox = inject(OmniboxService);
}
