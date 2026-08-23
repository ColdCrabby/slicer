import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { TwoFingerGesture, ViewerControl } from '../../services/viewer-control';
import { SectionHeader } from '../../ui/section-header/section-header';

@Component({
  selector: 'nexus-settings-general',
  imports: [SectionHeader],
  templateUrl: './general.html',
  styleUrl: './general.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class GeneralSettings {
  protected readonly viewer = inject(ViewerControl);
  protected readonly gesture = this.viewer.trackpadTwoFingerGesture;

  protected readonly version = '0.1.0';
  protected readonly platform =
    typeof globalThis !== 'undefined' &&
    ('__TAURI_INTERNALS__' in globalThis || '__TAURI__' in globalThis)
      ? 'Desktop'
      : 'Web';

  setGesture(gesture: TwoFingerGesture): void {
    this.viewer.setTrackpadTwoFingerGesture(gesture);
  }
}
