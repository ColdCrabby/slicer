import { ChangeDetectionStrategy, Component } from '@angular/core';
import { RouterLink, RouterLinkActive } from '@angular/router';
import { Icon } from '@coldcrabby/ui';

interface NavItem {
  path: string;
  label: string;
  icon: string;
  exact: boolean;
}

/**
 * Slim, always-visible primary navigation rail. Lives in the global app shell
 * so it is available on every surface (dashboard, slice workspace, settings).
 */
@Component({
  selector: 'nexus-nav-rail',
  imports: [RouterLink, RouterLinkActive, Icon],
  templateUrl: './nav-rail.html',
  styleUrl: './nav-rail.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class NavRail {
  protected readonly items: NavItem[] = [
    { path: '/', label: 'Home', icon: 'home-simple', exact: true },
    { path: '/slice', label: 'Slice', icon: 'box-iso', exact: false },
    { path: '/settings', label: 'Settings', icon: 'settings', exact: false },
  ];
}
