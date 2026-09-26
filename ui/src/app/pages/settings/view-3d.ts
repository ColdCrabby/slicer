import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import {
  MAX_FIELD_OF_VIEW,
  MIN_FIELD_OF_VIEW,
  ViewerControl,
  type Antialiasing,
  type ModelShading,
  type PreviewDetail,
  type RenderQuality,
} from '../../services/viewer-control';
import { SectionHeader, Segmented, Slider, Switch, type SegmentOption } from '@coldcrabby/ui';
import { FovCube } from '../../ui/fov-cube/fov-cube';
import { PrefRow } from './prefs/pref-row';
import { landOnFragment } from './prefs/land-on-fragment';

/**
 * How the 3D view looks and what it spends to look that way.
 *
 * Split out of General, where these eleven rows sat under "3D View" in a single
 * card between the backup button and the version number. Grouped here by the
 * question a reader arrives with — how it looks, how the camera frames it, what
 * it costs, what the embedded thumbnail looks like — rather than in the order
 * they were added.
 */
@Component({
  selector: 'nexus-settings-view-3d',
  imports: [SectionHeader, Segmented, Slider, Switch, FovCube, PrefRow],
  templateUrl: './view-3d.html',
  styleUrl: './view-3d.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class View3dSettings {
  protected readonly viewer = inject(ViewerControl);

  protected readonly minFov = MIN_FIELD_OF_VIEW;
  protected readonly maxFov = MAX_FIELD_OF_VIEW;

  protected readonly shadingOptions: SegmentOption[] = [
    { value: 'smooth', label: 'Smooth' },
    { value: 'flat', label: 'Flat' },
  ];

  protected readonly antialiasingOptions: SegmentOption[] = [
    { value: 'auto', label: 'Auto' },
    { value: 'on', label: 'On' },
    { value: 'off', label: 'Off' },
  ];

  protected readonly renderQualityOptions: SegmentOption[] = [
    { value: 'performance', label: 'Performance' },
    { value: 'balanced', label: 'Balanced' },
    { value: 'quality', label: 'Quality' },
  ];

  protected readonly previewDetailOptions: SegmentOption[] = [
    { value: 'auto', label: 'Auto' },
    { value: 'performance', label: 'Performance' },
    { value: 'quality', label: 'Quality' },
  ];

  protected readonly thumbnailLookOptions: SegmentOption[] = [
    { value: 'plain', label: 'Plain' },
    { value: 'scene', label: 'Match this view' },
  ];

  constructor() {
    landOnFragment();
  }

  protected setShading(mode: string): void {
    this.viewer.setModelShading(mode as ModelShading);
  }

  protected setAntialiasing(mode: string): void {
    this.viewer.setAntialiasing(mode as Antialiasing);
  }

  protected setRenderQuality(quality: string): void {
    this.viewer.setRenderQuality(quality as RenderQuality);
  }

  protected setPreviewDetail(detail: string): void {
    this.viewer.setPreviewDetail(detail as PreviewDetail);
  }

  protected setThumbnailLook(look: string): void {
    this.viewer.setThumbnailSceneEffects(look === 'scene');
  }
}
