import { computed, inject, Injectable, signal } from '@angular/core';
import { DOCUMENT } from '@angular/common';
import { Router } from '@angular/router';
import { firstValueFrom } from 'rxjs';
import { environment } from '../../environments/environment';
import { AppInfo, ChangelogEntry, SceneEngine } from './scene-engine';
import { BrowserStorage } from './browser-storage';
import { Dialog } from './dialog';
import { Logger } from './logger';

/** localStorage key holding the last release version the user has seen. */
const LAST_SEEN_KEY = 'slicer:last-seen-version';

/** Route showing the full release history. */
const CHANGELOG_ROUTE = '/settings/changelog';

/**
 * Changelog heading a development build is "on" — its work is by definition
 * still unreleased, so that is the section to highlight.
 */
const UNRELEASED = 'Unreleased';

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
 * last one recorded in localStorage and, when the user has upgraded, shows the
 * release history once with the new version highlighted. Development builds
 * (`is_release === false`) are never nagged.
 */
@Injectable({ providedIn: 'root' })
export class AppVersion {
  private readonly sceneEngine = inject(SceneEngine);
  private readonly storage = inject(BrowserStorage);
  private readonly dialog = inject(Dialog);
  private readonly router = inject(Router);
  private readonly log = inject(Logger).scope('AppVersion');
  private readonly document = inject(DOCUMENT);

  /** Guards {@link startUpdateWatch} against installing duplicate listeners. */
  private watchStarted = false;

  /** Timestamp of the last deploy-manifest fetch, for throttling. */
  private lastDeployCheck = 0;

  /** Build-time version metadata, populated after {@link checkForNewVersion}. */
  readonly info = signal<AppInfo | null>(null);

  /**
   * The embedded changelog, newest release first, with empty sections dropped
   * (a freshly cut release leaves `## [Unreleased]` with no body).
   */
  readonly changelog = signal<ChangelogEntry[]>([]);

  /**
   * Which changelog heading the running build corresponds to, for highlighting
   * in {@link ChangelogList}. Development builds map to `Unreleased`, since
   * their work has not shipped under a version number yet.
   */
  readonly currentChangelogVersion = computed(() => {
    const info = this.info();
    if (!info) {
      return null;
    }
    return info.is_release ? info.version : UNRELEASED;
  });

  /**
   * True when a newer build has been detected than the one this tab is running
   * — the app was (re)deployed while this tab kept an old bundle alive.
   * Surfaced by the reload prompt so the user can pick up the new version
   * without knowing to hard-refresh themselves.
   *
   * Two independent detectors set this: {@link reportServerVersion} (cloud/WS
   * deployments, from the server's announced version) and
   * {@link checkDeployedVersion} (static/Pages deployments, from a published
   * `version.json`). Either is sufficient. {@link reportStaleAssets} raises it
   * a third way, from a chunk that could no longer be fetched.
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
   * Ensure {@link changelog} is populated, reading the notes embedded in the
   * WASM bundle on first call. Subsequent calls are no-ops and failures are
   * logged, not thrown.
   */
  async loadChangelog(): Promise<void> {
    if (this.changelog().length > 0) {
      return;
    }
    try {
      const entries = await this.sceneEngine.changelogEntries();
      this.changelog.set(entries.filter((entry) => entry.body.trim().length > 0));
    } catch (err) {
      this.log.warn('Unable to read changelog', err);
    }
  }

  /**
   * Detect an upgrade and, if found, show the release history once.
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

    // Record before showing so a page refresh doesn't reopen the dialog even
    // if the user dismisses it without reading.
    this.storage.write(LAST_SEEN_KEY, info.version);

    await this.showWhatsNew(info.version);
  }

  /**
   * Surface the release notes for `version`, choosing the presentation the host
   * can actually render.
   *
   * Where dialogs are drawn by the OS (iOS/iPadOS) a `UIAlertController` takes a
   * title and a message and nothing richer, so the changelog cannot live inside
   * it. There the prompt is a short native confirm that hands the user off to
   * the settings page instead. Everywhere else the same `ChangelogList` the page
   * uses is embedded directly in the dialog.
   *
   * That component is imported *dynamically*. This service is `providedIn:
   * 'root'` and constructed during startup, so a static import would make its
   * markdown renderer part of the initial bundle for the sake of a dialog most
   * sessions never see.
   */
  private async showWhatsNew(version: string): Promise<void> {
    if (this.dialog.usesNativeDialogs()) {
      try {
        const view = await firstValueFrom(
          this.dialog.confirm({
            title: `Updated to ${version}`,
            message: 'See everything that changed in this release?',
            confirmLabel: "What's new",
            cancelLabel: 'Not now',
          }),
        );
        if (view) {
          await this.router.navigateByUrl(CHANGELOG_ROUTE);
        }
      } catch (err) {
        this.log.warn('Unable to prompt for release notes', err);
      }
      return;
    }

    const [{ ChangelogList }] = await Promise.all([
      import('../components/changelog/changelog-list'),
      this.loadChangelog(),
    ]);

    this.dialog.alert({
      title: `What's New in ${version}`,
      confirmLabel: 'Got it',
      content: ChangelogList,
      contentInputs: {
        entries: this.changelog(),
        currentVersion: version,
      },
      preferredWidth: '680px',
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
   * Flag this tab as running assets the server no longer serves.
   *
   * Called when a lazily-loaded chunk fails to arrive (see
   * {@link NavigationProgress}). The bundle's hashed filenames are pinned at
   * build time, so a redeploy under a long-lived tab leaves it asking for files
   * that have been swept away — every screen it has not visited yet is
   * unreachable until it reloads.
   *
   * No version is claimed here: the failure says the assets moved, not what
   * they moved to, and the banner reads fine without one. Idempotent, so a user
   * clicking through several broken links only ever sees one prompt.
   */
  reportStaleAssets(): void {
    if (this.updateAvailable()) {
      return;
    }
    this.log.info('An app chunk could no longer be fetched — prompting for a reload');
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
}

/** True when a changelog heading names a concrete release (not "Unreleased"). */
function isReleaseVersion(version: string): boolean {
  return /^\d+\.\d+\.\d+/.test(version.trim());
}
