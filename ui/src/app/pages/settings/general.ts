import { ChangeDetectionStrategy, Component, computed, inject, OnInit } from '@angular/core';
import {
  MAX_FIELD_OF_VIEW,
  MIN_FIELD_OF_VIEW,
  ViewerControl,
  type Antialiasing,
  type RenderQuality,
  type TwoFingerGesture,
} from '../../services/viewer-control';
import { AppVersion } from '../../services/app-version';
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
export class GeneralSettings implements OnInit {
  protected readonly viewer = inject(ViewerControl);
  private readonly appVersion = inject(AppVersion);
  protected readonly gesture = this.viewer.trackpadTwoFingerGesture;
  protected readonly statsVisible = this.viewer.statsVisible;
  protected readonly fieldOfView = this.viewer.fieldOfView;
  protected readonly antialiasing = this.viewer.antialiasing;
  protected readonly renderQuality = this.viewer.renderQuality;

  protected readonly minFov = MIN_FIELD_OF_VIEW;
  protected readonly maxFov = MAX_FIELD_OF_VIEW;

  /** Build-time version metadata read from the WASM bundle (SSOT). */
  protected readonly info = this.appVersion.info;

  /** The user-facing version — a release semver or `"development"`. */
  protected readonly version = computed(() => this.info()?.version ?? '…');

  /**
   * The exact commit the running build was cut from. Shown so deployed builds
   * can be pinned to a precise source revision, not just an official version.
   */
  protected readonly commit = computed(() => this.info()?.git_sha ?? '');

  protected readonly platform =
    typeof globalThis !== 'undefined' &&
    ('__TAURI_INTERNALS__' in globalThis || '__TAURI__' in globalThis)
      ? 'Desktop'
      : 'Web';

  ngOnInit(): void {
    void this.appVersion.loadInfo();
  }

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
