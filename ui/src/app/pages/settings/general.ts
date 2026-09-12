import { ChangeDetectionStrategy, Component, computed, inject, OnInit } from '@angular/core';
import { RouterLink } from '@angular/router';
import {
  MAX_FIELD_OF_VIEW,
  MIN_FIELD_OF_VIEW,
  ViewerControl,
  type Antialiasing,
  type ModelShading,
  type PreviewDetail,
  type PreviewFollow,
  type RenderQuality,
  type TwoFingerGesture,
} from '../../services/viewer-control';
import { ProfileExportButton } from '../../components/profiles/profile-export-button';
import {
  isTauriDesktop,
  isTauriMobile,
  resolveRuntimeMode,
} from '../../runtime/domain/runtime-mode.util';
import { formatDuration } from '../../models/duration';
import { AppVersion } from '../../services/app-version';
import { AutoSlice, type AutoSliceMode } from '../../services/auto-slice';
import {
  HistoryControlsPreference,
  type HistoryControlsMode,
} from '../../services/history-controls-preference';
import { Button, SectionHeader, Slider } from '@coldcrabby/ui';
import { FovCube } from '../../ui/fov-cube/fov-cube';
import {
  SettingsDetailPreference,
  type SettingsDetailMode,
} from '../../services/settings-detail-preference';

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
  protected readonly historyControls = inject(HistoryControlsPreference);
  protected readonly settingsDetail = inject(SettingsDetailPreference);
  protected readonly autoSlice = inject(AutoSlice);
  protected readonly gesture = this.viewer.trackpadTwoFingerGesture;
  protected readonly statsVisible = this.viewer.statsVisible;
  protected readonly palmRejection = this.viewer.palmRejection;
  protected readonly fieldOfView = this.viewer.fieldOfView;
  protected readonly antialiasing = this.viewer.antialiasing;
  protected readonly renderQuality = this.viewer.renderQuality;
  protected readonly previewDetail = this.viewer.previewDetail;
  protected readonly previewFollow = this.viewer.previewFollow;
  protected readonly useFilamentColor = this.viewer.useFilamentColor;
  protected readonly shadowsEnabled = this.viewer.shadowsEnabled;
  protected readonly modelShading = this.viewer.modelShading;
  protected readonly glossEnabled = this.viewer.glossEnabled;
  protected readonly thumbnailCaptureFx = this.viewer.thumbnailCaptureFx;
  protected readonly thumbnailSceneEffects = this.viewer.thumbnailSceneEffects;

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

  /**
   * The evidence `Automatic` is deciding on right now, for the plate that is
   * open. Quoted live so a plate that has quietly stopped re-slicing itself
   * says why, instead of looking like the setting stopped working.
   */
  protected readonly autoSliceNote = computed(() => {
    const last = this.autoSlice.lastSliceMs();
    if (last === null) {
      return 'The open plate has not been sliced yet — Automatic starts out re-slicing and settles once it has timed one.';
    }
    const took = `The open plate last sliced in ${formatDuration(last)}`;
    if (this.autoSlice.mode() !== 'auto') {
      return `${took}.`;
    }
    return this.autoSlice.enabled()
      ? `${took}, so Automatic is re-slicing it on its own.`
      : `${took}, so Automatic is leaving it to the Slice button.`;
  });

  /** Build-time version metadata read from the WASM bundle (SSOT). */
  protected readonly info = this.appVersion.info;

  /** The user-facing version — a release semver or `"development"`. */
  protected readonly version = computed(() => this.info()?.version ?? '…');

  /**
   * The exact commit the running build was cut from. Shown so deployed builds
   * can be pinned to a precise source revision, not just an official version.
   */
  protected readonly commit = computed(() => this.info()?.git_sha ?? '');

  /** The short SHA (first 7 characters) of the build's commit. */
  protected readonly shortCommit = computed(() => {
    const sha = this.commit();
    return sha.length >= 7 ? sha.substring(0, 7) : sha;
  });

  /** Direct GitHub link to the commit. */
  protected readonly commitUrl = computed(() => {
    const sha = this.commit();
    return sha ? `https://github.com/ColdCrabby/slicer/commit/${sha}` : '';
  });

  /**
   * Which runtime this build is actually in.
   *
   * A bare "is Tauri present?" check reported **Desktop** on iPadOS, which is a
   * Tauri host with no desktop chrome at all — no window API, no native menus.
   * The distinction is the same one `isTauriDesktop` exists to make everywhere
   * else, so it is the one used here.
   */
  protected readonly platform = isTauriMobile() ? 'Mobile' : isTauriDesktop() ? 'Desktop' : 'Web';

  ngOnInit(): void {
    void this.appVersion.loadInfo();
  }

  setGesture(gesture: TwoFingerGesture): void {
    this.viewer.setTrackpadTwoFingerGesture(gesture);
  }

  setStatsVisible(value: boolean): void {
    this.viewer.setStatsVisible(value);
  }

  /** Choose how much detail the settings panels open at. */
  setSettingsDetail(mode: SettingsDetailMode): void {
    this.settingsDetail.setMode(mode);
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

  setShadowsEnabled(value: boolean): void {
    this.viewer.setShadowsEnabled(value);
  }

  setModelShading(mode: ModelShading): void {
    this.viewer.setModelShading(mode);
  }

  setGlossEnabled(value: boolean): void {
    this.viewer.setGlossEnabled(value);
  }

  setThumbnailCaptureFx(value: boolean): void {
    this.viewer.setThumbnailCaptureFx(value);
  }

  setThumbnailSceneEffects(value: boolean): void {
    this.viewer.setThumbnailSceneEffects(value);
  }

  setHistoryControls(mode: HistoryControlsMode): void {
    this.historyControls.setMode(mode);
  }

  setAutoSlice(mode: AutoSliceMode): void {
    this.autoSlice.setMode(mode);
  }

  setPreviewFollow(mode: PreviewFollow): void {
    this.viewer.setPreviewFollow(mode);
  }
}
