import { ChangeDetectionStrategy, Component, DestroyRef, inject } from '@angular/core';
import { NoticePill } from '../notice-pill/notice-pill';
import { NotificationService, type Notice } from '../../../services/notifications';

/**
 * The scene's own voice: a centred strip of notices under the view toolbar.
 *
 * This is where anything about the plate belongs — a model being added, a job
 * uploading to a printer, a plate someone else just changed. It sits over the
 * work rather than in a corner of the window, so the message and the thing it
 * is about are in the same glance, and a finished job reports its outcome in
 * the very pill that was counting it up.
 *
 * Mounting it tells {@link NotificationService} that the scene exists; while it
 * does not, plate-scoped messages fall back to the window dock instead of being
 * rendered into a component that is not on screen.
 */
@Component({
  selector: 'nexus-scene-notices',
  standalone: true,
  imports: [NoticePill],
  templateUrl: './scene-notices.html',
  styleUrl: './scene-notices.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SceneNotices {
  readonly #service = inject(NotificationService);

  readonly notices = this.#service.sceneNotices;

  constructor() {
    inject(DestroyRef).onDestroy(this.#service.registerSceneHost());
  }

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
