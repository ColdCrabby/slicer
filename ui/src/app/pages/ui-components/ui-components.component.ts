import { RouterLink } from '@angular/router';
import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import type { OnDestroy } from '@angular/core';
import { Card } from '../../components/card/card';
import { Logo } from '../../components/logo/logo';
import { Dialog } from '../../services/dialog';
import { NotificationService } from '../../services/notifications';
import { Badge } from '../../shared/badge/badge';
import { Icon } from '../../shared/icon/icon';
import { RadioButtonValue } from '../../shared/radio-group/radio-button-value';
import { RadioGroup } from '../../shared/radio-group/radio-group';
import { TooltipDirective } from '../../shared/tooltip/tooltip.directive';
import { Button } from '../../ui/button/button';
import { EmptyState } from '../../ui/empty-state/empty-state';
import { IconButton } from '../../ui/icon-button/icon-button';
import { SectionHeader } from '../../ui/section-header/section-header';

@Component({
  selector: 'nexus-ui-components-page',
  standalone: true,
  imports: [
    RouterLink,
    SectionHeader,
    Button,
    IconButton,
    EmptyState,
    Badge,
    Icon,
    TooltipDirective,
    RadioGroup,
    RadioButtonValue,
    Card,
    Logo,
  ],
  templateUrl: './ui-components.component.html',
  styleUrl: './ui-components.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class UiComponentsPage implements OnDestroy {
  readonly #notifications = inject(NotificationService);
  readonly #dialog = inject(Dialog);

  #progressTimer: ReturnType<typeof setInterval> | null = null;

  ngOnDestroy(): void {
    if (this.#progressTimer !== null) {
      clearInterval(this.#progressTimer);
      this.#progressTimer = null;
    }
  }

  showInfoNotification(): void {
    this.#notifications.info('Heads up', 'A lightweight info message from the component lab.');
  }

  showSuccessNotification(): void {
    this.#notifications.success('Profile saved', 'Your current print profile is now active.');
  }

  showWarningNotification(): void {
    this.#notifications.warning('High speed selected', 'Layer adhesion may suffer at this speed.');
  }

  showErrorNotification(): void {
    this.#notifications.error('Slice failed', 'The server rejected the selected material profile.');
  }

  showProgressNotification(): void {
    if (this.#progressTimer !== null) {
      clearInterval(this.#progressTimer);
      this.#progressTimer = null;
    }

    const id = this.#notifications.progress('Slicing model', 'Preparing geometry...');
    let progress = 0;

    this.#progressTimer = setInterval(() => {
      progress += 10;

      if (progress >= 100) {
        const timer = this.#progressTimer;
        if (timer !== null) {
          clearInterval(timer);
        }
        this.#progressTimer = null;
        this.#notifications.completeProgress(
          id,
          'Slicing complete',
          'Preview is ready in the viewer.',
        );
        return;
      }

      this.#notifications.updateProgress(id, progress, `Slicing... ${progress}%`);
    }, 220);
  }

  openConfirmDialog(): void {
    this.#dialog
      .confirm({
        title: 'Discard changes?',
        message: 'Unsaved edits to this profile will be lost.',
        type: 'warning',
        confirmLabel: 'Discard',
      })
      .subscribe((confirmed) => {
        if (confirmed) {
          this.#notifications.success('Changes discarded');
        }
      });
  }

  openAlertDialog(): void {
    this.#dialog
      .alert({
        title: 'Export complete',
        message: 'The G-code file was exported to your downloads folder.',
      })
      .subscribe(() => {
        this.#notifications.info('Alert dismissed');
      });
  }
}
