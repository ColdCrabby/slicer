import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { RouterLink, RouterLinkActive } from '@angular/router';
import { Icon } from '@coldcrabby/ui';
import { NavigationProgress } from '../../services/navigation-progress';

interface NavItem {
  path: string;
  label: string;
  icon: string;
  exact: boolean;
}

/**
 * Slim, always-visible primary navigation rail. Lives in the global app shell
 * so it is available on every surface (dashboard, slice workspace, settings).
 *
 * Each destination is a lazily-loaded chunk, so a click can outlast a frame.
 * The item being fetched is marked as pending, which answers the question the
 * shell-wide progress bar cannot: not just *that* the app is busy, but which
 * of the three places it is busy going to.
 */
@Component({
  selector: 'nexus-nav-rail',
  imports: [RouterLink, RouterLinkActive, Icon],
  templateUrl: './nav-rail.html',
  styleUrl: './nav-rail.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class NavRail {
  private readonly navigation = inject(NavigationProgress);

  protected readonly items: NavItem[] = [
    { path: '/', label: 'Home', icon: 'home-simple', exact: true },
    { path: '/slice', label: 'Slice', icon: 'box-iso', exact: false },
    { path: '/settings', label: 'Settings', icon: 'settings', exact: false },
  ];

  /** Whether this destination is the one currently being loaded. */
  protected isPending(path: string): boolean {
    return this.navigation.isPendingUnder(path);
  }
}
