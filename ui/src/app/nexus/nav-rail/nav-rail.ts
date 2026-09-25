import { ChangeDetectionStrategy, Component, computed, effect, inject } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { NavigationEnd, Router, RouterLink, RouterLinkActive } from '@angular/router';
import { Icon } from '@coldcrabby/ui';
import { filter, map } from 'rxjs';
import { LibraryFlyout } from '../../services/library/library-flyout';
import { NavigationProgress } from '../../services/navigation-progress';

/** A plate's own page — `/slice/<uuid>`, not the new-plate screen. */
const PLATE_URL = /^\/slice\/(?!new(?:[/?#]|$))[^/?#]+/;

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
 * of the places it is busy going to.
 *
 * Library is the one destination that changes with where you are: with a plate
 * on screen it opens beside the plate instead of replacing it, since what the
 * user wants from the library then is something to put on that plate.
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
  private readonly router = inject(Router);
  protected readonly libraryFlyout = inject(LibraryFlyout);

  readonly #url = toSignal(
    this.router.events.pipe(
      filter((e) => e instanceof NavigationEnd),
      map(() => this.router.url),
    ),
    { initialValue: this.router.url },
  );
  /** A plate is on screen, so Library opens beside it. */
  protected readonly onPlate = computed(() => PLATE_URL.test(this.#url()));

  constructor() {
    // Leaving the plate takes its flyout with it.
    effect(() => {
      if (!this.onPlate()) {
        this.libraryFlyout.close();
      }
    });
  }

  protected readonly items: NavItem[] = [
    { path: '/', label: 'Home', icon: 'home-simple', exact: true },
    { path: '/slice', label: 'Slice', icon: 'box-iso', exact: false },
    { path: '/library', label: 'Library', icon: 'book-stack', exact: false },
    { path: '/settings', label: 'Settings', icon: 'settings', exact: false },
  ];

  /** Whether this destination is the one currently being loaded. */
  protected isPending(path: string): boolean {
    return this.navigation.isPendingUnder(path);
  }
}
