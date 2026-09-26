import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import {
  ViewerControl,
  type GcodeStepButtons,
  type TwoFingerGesture,
} from '../../services/viewer-control';
import { Viewport } from '../../services/viewport';
import {
  HistoryControlsPreference,
  type HistoryControlsMode,
} from '../../services/history-controls-preference';
import { SectionHeader, Segmented, Switch, type SegmentOption } from '@coldcrabby/ui';
import { PrefRow } from './prefs/pref-row';
import { landOnFragment } from './prefs/land-on-fragment';
import { ShortcutReference } from './shortcuts';

/**
 * How the app is driven: the trackpad and touch gestures, the on-screen
 * buttons that stand in for keys on a touchscreen, and the keys themselves.
 *
 * One page because it is one question — "how do I drive this?" — whichever
 * hand is answering it. The keyboard reference used to be a page of its own and
 * the gesture preferences were rows in General between the slicer and the 3D
 * view.
 */
@Component({
  selector: 'nexus-settings-controls',
  imports: [SectionHeader, Segmented, Switch, PrefRow, ShortcutReference],
  templateUrl: './controls.html',
  styleUrl: './controls.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ControlsSettings {
  protected readonly viewer = inject(ViewerControl);
  private readonly viewport = inject(Viewport);
  protected readonly historyControls = inject(HistoryControlsPreference);

  protected readonly gestureOptions: SegmentOption[] = [
    { value: 'orbit', label: 'Orbit' },
    { value: 'pan', label: 'Pan' },
  ];

  protected readonly historyOptions: SegmentOption[] = [
    { value: 'auto', label: 'Auto' },
    { value: 'always', label: 'Always' },
    { value: 'never', label: 'Never' },
  ];

  protected readonly gcodeStepOptions: SegmentOption[] = [
    { value: 'auto', label: 'Auto' },
    { value: 'on', label: 'On' },
    { value: 'off', label: 'Off' },
  ];

  /**
   * What `Auto` is doing for the step buttons, on this device. Quoted live so
   * someone on a touchscreen laptop — where the pointer, not the machine type,
   * decides — can see why the buttons are (or aren't) there.
   */
  protected readonly gcodeStepsNote = computed(() =>
    this.viewer.gcodeStepButtons() !== 'auto'
      ? ''
      : this.viewport.isCoarsePointer()
        ? 'Showing on this device — its pointer is a finger or a pencil.'
        : 'Hidden on this device — the arrow keys already reach the same steps.',
  );

  constructor() {
    landOnFragment();
  }

  protected setGesture(gesture: string): void {
    this.viewer.setTrackpadTwoFingerGesture(gesture as TwoFingerGesture);
  }

  protected setHistoryControls(mode: string): void {
    this.historyControls.setMode(mode as HistoryControlsMode);
  }

  protected setGcodeStepButtons(mode: string): void {
    this.viewer.setGcodeStepButtons(mode as GcodeStepButtons);
  }
}
