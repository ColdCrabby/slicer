import { ChangeDetectionStrategy, Component, computed, inject, OnInit } from '@angular/core';
import { RouterLink } from '@angular/router';
import { ViewerControl, type PreviewFollow } from '../../services/viewer-control';
import { ProfileExportButton } from '../../components/profiles/profile-export-button';
import {
  isTauriDesktop,
  isTauriMobile,
  resolveRuntimeMode,
} from '../../runtime/domain/runtime-mode.util';
import { formatDuration } from '../../models/duration';
import { AppVersion } from '../../services/app-version';
import { AutoSlice, type AutoSliceMode } from '../../services/auto-slice';
import { Button, Icon, SectionHeader, Segmented, type SegmentOption } from '@coldcrabby/ui';
import {
  SettingsDetailPreference,
  type SettingsDetailMode,
} from '../../services/settings-detail-preference';
import { PrefRow } from './prefs/pref-row';
import { landOnFragment } from './prefs/land-on-fragment';
import { storageNote } from './prefs/storage-note';

/**
 * How the app behaves as a whole: when it slices, how much the settings panels
 * show, where your library lives, and which build this is.
 *
 * It used to hold every app preference — twenty rows, each under a paragraph —
 * and the 3D view's look and the input controls now have pages of their own.
 */
@Component({
  selector: 'nexus-settings-general',
  imports: [Button, Icon, ProfileExportButton, RouterLink, SectionHeader, Segmented, PrefRow],
  templateUrl: './general.html',
  styleUrl: './general.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class GeneralSettings implements OnInit {
  private readonly viewer = inject(ViewerControl);
  private readonly appVersion = inject(AppVersion);
  protected readonly settingsDetail = inject(SettingsDetailPreference);
  protected readonly autoSlice = inject(AutoSlice);
  protected readonly previewFollow = this.viewer.previewFollow;

  protected readonly autoSliceOptions: SegmentOption[] = [
    { value: 'auto', label: 'Automatic' },
    { value: 'on', label: 'Always' },
    { value: 'off', label: 'Off' },
  ];

  protected readonly previewFollowOptions: SegmentOption[] = [
    { value: 'auto', label: 'Automatic' },
    { value: 'always', label: 'Always' },
    { value: 'never', label: 'Never' },
  ];

  protected readonly settingsDetailOptions: SegmentOption[] = [
    { value: 'everyday', label: 'Standard' },
    { value: 'advanced', label: 'Advanced' },
    { value: 'expert', label: 'Everything' },
  ];

  /** Where the library is kept in this runtime, and what that means. */
  protected readonly storage = storageNote();

  /**
   * Where the exported library comes from. Engine-backed runtimes export the
   * copy persisted next to the slicer — the one the CLI would read — while the
   * web runtime, where the browser is the engine, exports this browser's copy.
   */
  protected readonly exportHint =
    resolveRuntimeMode() === 'web'
      ? 'Printers, filaments, processes and labels as TOML — this browser’s copy.'
      : 'Printers, filaments, processes and labels as TOML — the copy saved with the slicer.';

  /**
   * The evidence `Automatic` is deciding on right now, for the plate that is
   * open. Quoted live so a plate that has quietly stopped re-slicing itself
   * says why, instead of looking like the setting stopped working. Empty for
   * the fixed choices, which have nothing to explain.
   */
  protected readonly autoSliceNote = computed(() => {
    if (this.autoSlice.mode() !== 'auto') {
      return '';
    }
    const last = this.autoSlice.lastSliceMs();
    if (last === null) {
      return 'Re-slicing until the open plate has been timed once.';
    }
    const took = formatDuration(last);
    return this.autoSlice.enabled()
      ? `The open plate slices in ${took}, so it re-slices on its own.`
      : `The open plate takes ${took}, so it waits for the Slice button.`;
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

  /** The build in one line: name, version, and where it is running. */
  protected readonly aboutLine = computed(() => `Cold Crabby ${this.version()} · ${this.platform}`);

  constructor() {
    landOnFragment();
  }

  ngOnInit(): void {
    void this.appVersion.loadInfo();
  }

  setSettingsDetail(mode: string): void {
    this.settingsDetail.setMode(mode as SettingsDetailMode);
  }

  setAutoSlice(mode: string): void {
    this.autoSlice.setMode(mode as AutoSliceMode);
  }

  setPreviewFollow(mode: string): void {
    this.viewer.setPreviewFollow(mode as PreviewFollow);
  }
}
