import { ChangeDetectionStrategy, Component, computed, inject, OnInit } from '@angular/core';
import { RouterLink } from '@angular/router';
import {
  MAX_FIELD_OF_VIEW,
  MIN_FIELD_OF_VIEW,
  ViewerControl,
  type Antialiasing,
  type PreviewDetail,
  type RenderQuality,
  type TwoFingerGesture,
} from '../../services/viewer-control';
import { ProfileExportButton } from '../../components/profiles/profile-export-button';
import { resolveRuntimeMode } from '../../runtime/domain/runtime-mode.util';
import { AppVersion } from '../../services/app-version';
import { Button, SectionHeader, Slider } from '@coldcrabby/ui';
import { FovCube } from '../../ui/fov-cube/fov-cube';

@Component({
  selector: 'nexus-settings-general',
  imports: [Button, ProfileExportButton, RouterLink, SectionHeader, Slider, FovCube],
  templateUrl: './general.html',
  styleUrl: './general.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class GeneralSettings implements OnInit {
  protected readonly viewer = inject(ViewerControl);
  private readonly appVersion = inject(AppVersion);
  protected readonly gesture = this.viewer.trackpadTwoFingerGesture;
  protected readonly statsVisible = this.viewer.statsVisible;
  protected readonly palmRejection = this.viewer.palmRejection;
  protected readonly fieldOfView = this.viewer.fieldOfView;
  protected readonly antialiasing = this.viewer.antialiasing;
  protected readonly renderQuality = this.viewer.renderQuality;
  protected readonly previewDetail = this.viewer.previewDetail;
  protected readonly useFilamentColor = this.viewer.useFilamentColor;

  protected readonly minFov = MIN_FIELD_OF_VIEW;
  protected readonly maxFov = MAX_FIELD_OF_VIEW;

  /**
   * Where the exported library comes from. Engine-backed runtimes export the
   * copy persisted next to the slicer — the one the CLI would read — while the
   * web runtime, where the browser is the engine, exports this browser's copy.
   */
  protected readonly exportScopeNote =
    resolveRuntimeMode() === 'web'
      ? 'Exports the library kept in this browser.'
      : 'Exports the library saved with the slicer.';

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

  setPalmRejection(value: boolean): void {
    this.viewer.setPalmRejection(value);
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

  setPreviewDetail(detail: PreviewDetail): void {
    this.viewer.setPreviewDetail(detail);
  }

  setUseFilamentColor(value: boolean): void {
    this.viewer.setUseFilamentColor(value);
  }
}
