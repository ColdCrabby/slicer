import { RouterLink } from '@angular/router';
import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import type { OnDestroy } from '@angular/core';
import { Card } from '../../components/card/card';
import { ConnectionState } from '../../components/connection-state/connection-state';
import { Logo } from '../../components/logo/logo';
import { Dialog } from '../../services/dialog';
import { NotificationService } from '../../services/notifications';
import { Badge } from '../../shared/badge/badge';
import { Icon } from '../../shared/icon/icon';
import { IconButton as SharedIconButton } from '../../shared/icon-button/icon-button';
import { RadioButtonValue } from '../../shared/radio-group/radio-button-value';
import { RadioGroup } from '../../shared/radio-group/radio-group';
import { StackWhenCramped } from '../../shared/radio-group/stack-when-cramped';
import { TooltipDirective } from '../../shared/tooltip/tooltip.directive';
import { Button } from '../../ui/button/button';
import { ColorPicker } from '../../ui/color-picker/color-picker';
import { EmptyState } from '../../ui/empty-state/empty-state';
import { IconButton } from '../../ui/icon-button/icon-button';
import { NumberInput } from '../../ui/number-input/number-input';
import { RadioGroup as NexusRadioGroup } from '../../ui/radio-group/radio-group';
import type { RadioOption } from '../../ui/radio-group/radio-group';
import { RangeSlider } from '../../ui/range-slider/range-slider';
import { SectionHeader } from '../../ui/section-header/section-header';
import { Segmented } from '../../ui/segmented/segmented';
import type { SegmentOption } from '../../ui/segmented/segmented';
import { Select } from '../../ui/select/select';
import type { SelectOption } from '../../ui/select/select';
import { Slider } from '../../ui/slider/slider';
import { Switch } from '../../ui/switch/switch';

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
    SharedIconButton,
    ConnectionState,
    TooltipDirective,
    RadioGroup,
    RadioButtonValue,
    StackWhenCramped,
    Card,
    Logo,
    Switch,
    Slider,
    RangeSlider,
    NumberInput,
    Select,
    NexusRadioGroup,
    Segmented,
    ColorPicker,
  ],
  templateUrl: './ui-components.component.html',
  styleUrl: './ui-components.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class UiComponentsPage implements OnDestroy {
  readonly #notifications = inject(NotificationService);
  readonly #dialog = inject(Dialog);

  // --- Form-control demo state -------------------------------------------
  protected readonly supportsOn = signal(true);
  protected readonly spiralOn = signal(false);
  protected readonly ironingOn = signal(false);
  protected readonly density = signal(20);
  protected readonly speed = signal(120);
  protected readonly tempLow = signal(190);
  protected readonly tempHigh = signal(230);
  protected readonly layerRange = signal<[number, number]>([12, 84]);
  protected readonly layerHeight = signal(0.2);
  protected readonly wallCount = signal(3);
  protected readonly pattern = signal('grid');
  protected readonly wallGenerator = signal('arachne');
  protected readonly qualityMode = signal('balanced');
  protected readonly filamentColor = signal('#e0730f');

  protected readonly qualityOptions: readonly SegmentOption[] = [
    { value: 'draft', label: 'Draft', description: 'Fast, coarse layers' },
    { value: 'balanced', label: 'Balanced', description: 'A sensible default' },
    { value: 'detail', label: 'Detail', description: 'Fine layers, slower' },
  ];

  protected readonly wallGeneratorOptions: readonly RadioOption[] = [
    { value: 'classic', label: 'Classic', description: 'Fixed-width concentric perimeters' },
    { value: 'arachne', label: 'Arachne', description: 'Variable-width beads for thin walls' },
  ];

  protected readonly patternOptions: readonly SelectOption[] = [
    { value: 'grid', label: 'Grid', description: 'Fast, strong, two-directional' },
    { value: 'gyroid', label: 'Gyroid', description: 'Isotropic, flexible, slow' },
    { value: 'honeycomb', label: 'Honeycomb', description: 'High strength-to-weight' },
    { value: 'rectilinear', label: 'Rectilinear', description: 'Simple back-and-forth lines' },
    { value: 'tpms-d', label: 'TPMS Diamond', description: 'Smooth minimal surface' },
  ];

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

  openWarningConfirmDialog(): void {
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

  openDangerConfirmDialog(): void {
    this.#dialog
      .confirm({
        title: 'Delete printer "Voron 2.4"?',
        message: 'This printer profile will be permanently deleted.',
        type: 'danger',
        confirmLabel: 'Delete',
      })
      .subscribe((confirmed) => {
        if (confirmed) {
          this.#notifications.warning(
            'Delete confirmed',
            'Demo only: this action does not remove anything.',
          );
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
