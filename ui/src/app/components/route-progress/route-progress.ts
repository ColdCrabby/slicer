import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { NavigationProgress } from '../../services/navigation-progress';

/**
 * A hairline bar that admits the app is fetching the screen you asked for.
 *
 * Every routed surface is a lazily-loaded chunk, so a navigation can involve a
 * real download. Angular keeps the *old* screen on display until the new one
 * resolves, which is indistinguishable from a click that did nothing. This is
 * the app's answer to "is it working?" — the same hairline the browser itself
 * uses, in the app's accent, so it reads as chrome rather than content.
 *
 * It is deliberately **indeterminate**: a module fetch has no meaningful
 * percentage, and a fake one that jumps to 90 % and waits is a lie. The bar
 * sweeps while the work is unknown, then fills once and fades on completion.
 *
 * Purely presentational — {@link NavigationProgress} owns every decision about
 * *when* it appears, including the delay that keeps instant navigations silent.
 */
@Component({
  selector: 'nexus-route-progress',
  templateUrl: './route-progress.html',
  styleUrl: './route-progress.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class RouteProgress {
  private readonly progress = inject(NavigationProgress);

  protected readonly active = this.progress.active;
  protected readonly complete = this.progress.complete;
}
