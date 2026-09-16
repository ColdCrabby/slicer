import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { NoticePill } from '../notice-pill/notice-pill';
import { NotificationService, type Notice } from '../../../services/notifications';

/**
 * The window's dock: bottom-centre, for the few messages that are not about a
 * plate.
 *
 * Two kinds land here. Things that are genuinely app-level — a newer build
 * waiting to be loaded, an engine that cannot be reached — and things that
 * *would* have gone to the scene but were raised while no scene was mounted,
 * such as a model the OS handed us during a cold launch.
 *
 * Bottom-**centre**, deliberately: the bottom-left of the scene belongs to the
 * object list and the build-area warning, and a stack of floating messages over
 * them covered the two things most worth reading.
 */
@Component({
  selector: 'nexus-app-notices',
  standalone: true,
  imports: [NoticePill],
  templateUrl: './app-notices.html',
  styleUrl: './app-notices.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AppNotices {
  readonly #service = inject(NotificationService);

  readonly notices = this.#service.windowNotices;

  protected act(notice: Notice): void {
    this.#service.act(notice.id);
  }

  protected dismiss(notice: Notice): void {
    this.#service.dismiss(notice.id);
  }

  protected hold(notice: Notice): void {
    this.#service.pauseAutoDismiss(notice.id);
  }

  protected release(notice: Notice): void {
    this.#service.resumeAutoDismiss(notice.id);
  }

  protected trackById(_index: number, notice: Notice): string {
    return notice.id;
  }
}
