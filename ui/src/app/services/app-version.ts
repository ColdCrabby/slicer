import { inject, Injectable, signal } from '@angular/core';
import { DOCUMENT } from '@angular/common';
import { environment } from '../../environments/environment';
import { WhatsNewPanel } from '../components/whats-new/whats-new-panel';
import { AppInfo, ChangelogEntry, SceneEngine } from './scene-engine';
import { BrowserStorage } from './browser-storage';
import { Dialog } from './dialog';
import { Logger } from './logger';

/** localStorage key holding the last release version the user has seen. */
const LAST_SEEN_KEY = 'slicer:last-seen-version';

/**
 * Static manifest published next to the bundle at deploy time (see the Pages
 * workflow). Its `sha` is the git commit the deployment was built from — the
 * same value baked into the running bundle via `appInfo().git_sha`.
 */
interface DeployManifest {
  sha: string;
  version?: string;
}

/** Path (relative to the app base href) of the deploy manifest. */
const DEPLOY_MANIFEST_PATH = 'version.json';

/** Minimum gap between deploy-manifest network checks, to avoid hammering. */
const DEPLOY_CHECK_THROTTLE_MS = 60_000;

/** How often to poll the deploy manifest for a tab that stays visible. */
const DEPLOY_POLL_INTERVAL_MS = 15 * 60_000;

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
  private readonly document = inject(DOCUMENT);

  /** Guards {@link startUpdateWatch} against installing duplicate listeners. */
  private watchStarted = false;

  /** Timestamp of the last deploy-manifest fetch, for throttling. */
  private lastDeployCheck = 0;

  /** Build-time version metadata, populated after {@link checkForNewVersion}. */
  readonly info = signal<AppInfo | null>(null);

  /** Changelog sections to render in the What's New dialog body. */
  readonly whatsNew = signal<ChangelogEntry[]>([]);

  /**
   * True when a newer build has been detected than the one this tab is running
   * — the app was (re)deployed while this tab kept an old bundle alive.
   * Surfaced by the reload prompt so the user can pick up the new version
   * without knowing to hard-refresh themselves.
   *
   * Two independent detectors set this: {@link reportServerVersion} (cloud/WS
   * deployments, from the server's announced version) and
   * {@link checkDeployedVersion} (static/Pages deployments, from a published
   * `version.json`). Either is sufficient.
   */
  readonly updateAvailable = signal(false);

  /**
   * A human-friendly label for the newer version that triggered
   * {@link updateAvailable}, when one is known and looks like a real release.
   * Stays `null` for untagged/development deploys (whose only reliable
   * difference is the git SHA, not a version string).
   */
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
   * Begin watching for a newer static deployment (Pages/`web` runtime).
   *
   * There is no server to announce a version in `web` mode — the bundle and the
   * WASM slicer are the same set of static files. Instead the deployment
   * publishes a `version.json` whose git SHA is re-fetched here and compared to
   * the SHA baked into the running bundle. A mismatch means a newer build has
   * gone live while this tab stayed open.
   *
   * Checks run once now and again whenever the tab regains visibility (the
   * moment a user returns to a long-idle tab), throttled to one network hit per
   * minute. Idempotent and a no-op outside `web` mode. Safe to call at startup.
   */
  startUpdateWatch(): void {
    if (this.watchStarted || environment.runtimeMode !== 'web') {
      return;
    }
    this.watchStarted = true;

    void this.checkDeployedVersion();

    this.document.addEventListener('visibilitychange', () => {
      if (this.document.visibilityState === 'visible') {
        void this.checkDeployedVersion();
      }
    });

    // Also poll on a slow timer so a tab that stays visible for hours still
    // notices a deploy. The per-check throttle keeps this cheap.
    setInterval(() => void this.checkDeployedVersion(), DEPLOY_POLL_INTERVAL_MS);
  }

  /**
   * Fetch the deploy manifest and flag {@link updateAvailable} when its git SHA
   * differs from the running bundle's. Throttled and completely silent on
   * failure — a missing/unreachable manifest (local dev, non-Pages hosting)
   * simply means "no update information", never an error.
   */
  async checkDeployedVersion(): Promise<void> {
    if (this.updateAvailable()) {
      return; // Already prompting — nothing more to learn.
    }

    const now = Date.now();
    if (now - this.lastDeployCheck < DEPLOY_CHECK_THROTTLE_MS) {
      return;
    }
    this.lastDeployCheck = now;

    await this.loadInfo();
    const runningSha = this.info()?.git_sha;
    if (!runningSha || runningSha === 'unknown') {
      return; // Nothing trustworthy to compare against.
    }

    const manifest = await this.fetchDeployManifest();
    if (!manifest?.sha || manifest.sha === runningSha) {
      return;
    }

    this.log.info(
      `Deployed build ${manifest.sha} differs from running ${runningSha} — a reload is needed`,
    );
    const label = manifest.version?.replace(/^v/, '');
    this.serverVersion.set(label && isReleaseVersion(label) ? label : null);
    this.updateAvailable.set(true);
  }

  /**
   * Fetch and parse the deploy manifest, bypassing the HTTP cache so a stale
   * tab still sees the freshly deployed file. Returns `null` on any failure.
   */
  private async fetchDeployManifest(): Promise<DeployManifest | null> {
    try {
      const url = new URL(DEPLOY_MANIFEST_PATH, this.document.baseURI);
      url.searchParams.set('_', Date.now().toString(36));
      const res = await fetch(url.toString(), { cache: 'no-store' });
      if (!res.ok) {
        return null;
      }
      const data = (await res.json()) as Partial<DeployManifest>;
      return typeof data?.sha === 'string' ? { sha: data.sha, version: data.version } : null;
    } catch {
      return null;
    }
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
