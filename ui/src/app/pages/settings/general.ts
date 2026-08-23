import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import {
  MAX_FIELD_OF_VIEW,
  MIN_FIELD_OF_VIEW,
  ViewerControl,
  type Antialiasing,
  type RenderQuality,
  type TwoFingerGesture,
} from '../../services/viewer-control';
import { SectionHeader } from '../../ui/section-header/section-header';
import { Slider } from '../../ui/slider/slider';
import { FovCube } from '../../ui/fov-cube/fov-cube';

@Component({
  selector: 'nexus-settings-general',
  imports: [SectionHeader, Slider, FovCube],
  templateUrl: './general.html',
  styleUrl: './general.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class GeneralSettings {
  protected readonly viewer = inject(ViewerControl);
  protected readonly gesture = this.viewer.trackpadTwoFingerGesture;
  protected readonly statsVisible = this.viewer.statsVisible;
  protected readonly fieldOfView = this.viewer.fieldOfView;
  protected readonly antialiasing = this.viewer.antialiasing;
  protected readonly renderQuality = this.viewer.renderQuality;

  protected readonly minFov = MIN_FIELD_OF_VIEW;
  protected readonly maxFov = MAX_FIELD_OF_VIEW;

  protected readonly version = '0.1.0';
  protected readonly platform =
    typeof globalThis !== 'undefined' &&
    ('__TAURI_INTERNALS__' in globalThis || '__TAURI__' in globalThis)
      ? 'Desktop'
      : 'Web';

  setGesture(gesture: TwoFingerGesture): void {
    this.viewer.setTrackpadTwoFingerGesture(gesture);
  }

  setStatsVisible(value: boolean): void {
    this.viewer.setStatsVisible(value);
  }

  setFieldOfView(value: number): void {
    this.viewer.setFieldOfView(value);
  }

  setAntialiasing(mode: Antialiasing): void {
    this.viewer.setAntialiasing(mode);
  }

  setRenderQuality(quality: RenderQuality): void {
    this.viewer.setRenderQuality(quality);
  }
}
