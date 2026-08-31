import { ChangeDetectionStrategy, Component } from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { RouteProgress } from '../../components/route-progress/route-progress';
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
 */
@Component({
  selector: 'nexus-shell',
  imports: [RouterOutlet, NexusTitlebar, NavRail, RouteProgress],
  templateUrl: './shell.html',
  styleUrl: './shell.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AppShell {}
