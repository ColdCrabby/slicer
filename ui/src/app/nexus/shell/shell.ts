import { ChangeDetectionStrategy, Component } from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { NavRail } from '../nav-rail/nav-rail';
import { NexusTitlebar } from '../titlebar/titlebar';

/**
 * Global application shell: the custom title bar on top, a primary navigation
 * rail on the left, and the routed surface (dashboard / slice workspace /
 * settings) filling the rest. Wraps every route.
 */
@Component({
  selector: 'nexus-shell',
  imports: [RouterOutlet, NexusTitlebar, NavRail],
  templateUrl: './shell.html',
  styleUrl: './shell.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AppShell {}
