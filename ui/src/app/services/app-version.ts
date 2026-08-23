import { inject, Injectable, signal } from '@angular/core';
import { WhatsNewPanel } from '../components/whats-new/whats-new-panel';
import { AppInfo, ChangelogEntry, SceneEngine } from './scene-engine';
import { BrowserStorage } from './browser-storage';
import { Dialog } from './dialog';
import { Logger } from './logger';

/** localStorage key holding the last release version the user has seen. */
const LAST_SEEN_KEY = 'slicer:last-seen-version';

/**
 * Owns the app's version identity and the "What's New" upgrade experience.
 *
 * The running version and changelog are read straight from the WASM bundle
 * (see {@link SceneEngine.appInfo}) so the same numbers baked into the binary
 * drive the UI — there is no separate, drift-prone frontend version constant.
 *
 * On startup {@link checkForNewVersion} compares the running release against the
 * last one recorded in localStorage and, when the user has upgraded, opens a
 * one-time dialog with the notes for every release they skipped. Development
 * builds (`is_release === false`) are never nagged.
 */
@Injectable({ providedIn: 'root' })
export class AppVersion {
  private readonly sceneEngine = inject(SceneEngine);
  private readonly storage = inject(BrowserStorage);
  private readonly dialog = inject(Dialog);
  private readonly log = inject(Logger).scope('AppVersion');

  /** Build-time version metadata, populated after {@link checkForNewVersion}. */
  readonly info = signal<AppInfo | null>(null);

  /** Changelog sections to render in the What's New dialog body. */
  readonly whatsNew = signal<ChangelogEntry[]>([]);

  /**
   * True when the server announced a release version that differs from the one
   * baked into the running UI bundle — i.e. the app was redeployed while this
   * tab kept an old build alive. Surfaced by the reload prompt so the user can
   * pick up the new version without knowing to hard-refresh themselves.
   */
  readonly updateAvailable = signal(false);

  /** The newer server version that triggered {@link updateAvailable}, if any. */
  readonly serverVersion = signal<string | null>(null);

  /**
   * Ensure {@link info} is populated, loading it from the WASM bundle on first
   * call. Safe to call from any component that wants to display the running
   * version; subsequent calls are no-ops. Failures are logged, not thrown.
   */
  async loadInfo(): Promise<void> {
    if (this.info()) {
      return;
    }
    try {
      this.info.set(await this.sceneEngine.appInfo());
    } catch (err) {
      this.log.warn('Unable to read app version', err);
    }
  }

  /**
   * Detect an upgrade and, if found, show the "What's New" dialog once.
   * Safe to call during app initialization — failures are logged, not thrown.
   */
  async checkForNewVersion(): Promise<void> {
    let info: AppInfo;
    try {
      info = await this.sceneEngine.appInfo();
    } catch (err) {
      this.log.warn('Unable to read app version', err);
      return;
    }
    this.info.set(info);

    // Development builds have an unstable "development" version — never nag.
    if (!info.is_release) {
      return;
    }

    const lastSeen = this.storage.get(LAST_SEEN_KEY)();

    // First launch on this machine: record silently, don't surface notes.
    if (!lastSeen) {
      this.storage.write(LAST_SEEN_KEY, info.version);
      return;
    }

    if (lastSeen === info.version) {
      return;
    }

    const notes = await this.collectNotes(lastSeen, info.version);
    this.whatsNew.set(notes);

    // Record before showing so a page refresh doesn't reopen the dialog even
    // if the user dismisses it without reading.
    this.storage.write(LAST_SEEN_KEY, info.version);

    this.dialog.alert({
      title: `What's New in ${info.version}`,
      confirmLabel: 'Got it',
      content: WhatsNewPanel,
      preferredWidth: '640px',
    });
  }

  /**
   * Compare the version the server announced on (re)connect against the version
   * baked into the running UI bundle. When a real release differs from what this
   * tab is running, flag {@link updateAvailable} so the reload prompt appears.
   *
   * Only fires for release↔release mismatches: development builds (either side)
   * have an unstable `"development"` version and are never nagged. Safe to call
   * on every reconnect — failures are logged, not thrown.
   */
  async reportServerVersion(serverVersion: string | undefined): Promise<void> {
    if (!serverVersion || !isReleaseVersion(serverVersion)) {
      return;
    }

    await this.loadInfo();
    const running = this.info();

    // Can't compare without our own version, and never nag development builds.
    if (!running || !running.is_release || !isReleaseVersion(running.version)) {
      return;
    }

    if (running.version === serverVersion) {
      return;
    }

    this.log.info(
      `Server is on ${serverVersion} but this UI is running ${running.version} — a reload is needed`,
    );
    this.serverVersion.set(serverVersion);
    this.updateAvailable.set(true);
  }

  /**
   * Force a fresh load of the app, bypassing the browser cache so the newly
   * deployed bundle is fetched instead of the stale one this tab holds.
   */
  reloadForUpdate(): void {
    // A cache-busting query param guarantees the document (and thus its module
    // graph) is re-fetched even behind an over-eager HTTP cache.
    const url = new URL(window.location.href);
    url.searchParams.set('_v', Date.now().toString(36));
    window.location.replace(url.toString());
  }

  /**
   * Changelog entries for every release strictly newer than `lastSeen` and no
   * newer than `current`. Falls back to just the current version's entry when
   * nothing matches (e.g. an unparseable stored version).
   */
  private async collectNotes(lastSeen: string, current: string): Promise<ChangelogEntry[]> {
    let entries: ChangelogEntry[] = [];
    try {
      entries = await this.sceneEngine.changelogEntries();
    } catch (err) {
      this.log.warn('Unable to read changelog', err);
      return [];
    }

    const notes = entries.filter(
      (e) =>
        isReleaseVersion(e.version) &&
        compareSemver(e.version, lastSeen) > 0 &&
        compareSemver(e.version, current) <= 0,
    );

    if (notes.length > 0) {
      return notes;
    }

    return entries.filter((e) => e.version === current);
  }
}

/** True when a changelog heading names a concrete release (not "Unreleased"). */
function isReleaseVersion(version: string): boolean {
  return /^\d+\.\d+\.\d+/.test(version.trim());
}

/**
 * Compare two dotted version strings numerically (major.minor.patch), ignoring
 * any pre-release/build suffix. Returns <0, 0, or >0.
 */
function compareSemver(a: string, b: string): number {
  const parse = (v: string): number[] =>
    v
      .trim()
      .replace(/^v/, '')
      .split(/[.\-+]/)
      .map((n) => Number.parseInt(n, 10))
      .filter((n) => !Number.isNaN(n));

  const pa = parse(a);
  const pb = parse(b);
  const len = Math.max(pa.length, pb.length);

  for (let i = 0; i < len; i++) {
    const diff = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (diff !== 0) {
      return diff;
    }
  }
  return 0;
}
