# Releasing

This project has **one source of truth for releases: a git tag**. Everything
else — the version baked into every binary, the "What's New" dialog in the UI,
the published GitHub Release, and the attached artifacts — is derived from that
tag and from [CHANGELOG.md](CHANGELOG.md). There is no second place to bump a
version by hand.

## How versioning works

The running version is computed at **build time** by [`build.rs`](build.rs),
which probes git:

| Build situation                                   | Reported version |
| ------------------------------------------------- | ---------------- |
| Clean checkout sitting exactly on a `vX.Y.Z` tag  | `X.Y.Z`          |
| Any commit ahead of a tag, or a dirty working tree| `development`    |
| No tags at all (fresh clone)                      | `development`    |

That value is exposed to every target through
[`src/version.rs`](src/version.rs) (`crate::version::VERSION`) and surfaced by:

- the CLI — `slicer-engine --version` and `slicer-engine info`,
- the WebSocket server — the `Connected { version }` handshake,
- the WASM bundle — `appVersion()` / `appInfo()`, read by the Angular UI,
- the Tauri desktop app — via the same WASM bundle.

Because the version is honest by construction, local development builds always
read `development` instead of a stale, misleading number. Only a tagged, clean
release ever reports a real semver.

> The `version` field in `Cargo.toml` is the *next* target version the
> maintainers are working towards. It is **not** what users see — that always
> comes from the git tag.

## The changelog

[CHANGELOG.md](CHANGELOG.md) follows [Keep a Changelog](https://keepachangelog.com/)
and is embedded into every build via `include_str!`. The UI shows the notes for
the newly installed version in a one-time **"What's New"** dialog the first time
a user runs an upgraded release (development builds are never nagged).

We maintain it with a **hybrid** workflow: a script drafts the notes from git
history, then a human (or the [`release` skill](.github/skills/release/SKILL.md))
curates them into enthusiastic, contributor-aware notes before tagging.

## Cutting a release — the easy way

Run the **`release` skill** (say "cut a release" to the agent). It automates this
whole section: it gathers the commits and contributors since the last tag, curates
the `CHANGELOG.md` section in the project's voice — leading with the biggest
features and giving first-time contributors a real spotlight — then tags and pushes
once you approve. The manual steps below are what that skill performs, and remain
available if you prefer to do it by hand.

## Cutting a release — step by step

1. **Draft the notes from git history.**

   ```bash
   scripts/gen-changelog-draft.sh          # since the last v* tag
   scripts/gen-changelog-draft.sh v0.2.0   # or since an explicit tag
   scripts/release-contributors.sh         # contributors + first-timers
   ```

   The first script prints a categorised `## [Unreleased]` block (Added /
   Changed / Fixed / Documentation / Other). The second lists everyone who
   landed a change since the last tag and flags first-time contributors so they
   can be acknowledged. Both write nothing — copy the output as a starting point.

2. **Curate `CHANGELOG.md` by hand.** Fold the draft into the existing
   `## [Unreleased]` section: drop noise, merge related entries, and write for
   humans. Then promote it to a dated release heading and open a fresh
   `Unreleased` section above it:

   ```markdown
   ## [Unreleased]

   ## [0.2.0] - 2026-09-01

   ### Added
   - ...
   ```

3. **Commit the changelog.**

   ```bash
   git add CHANGELOG.md
   git commit -m "docs: changelog for 0.2.0"
   ```

4. **Tag and push.** The tag must be `vX.Y.Z` (optionally with a
   `-rc.1`-style suffix for pre-releases, which are published as GitHub
   pre-releases).

   ```bash
   git tag v0.2.0
   git push origin main --tags
   ```

That is the entire manual process. Pushing the tag triggers
[`.github/workflows/release.yml`](.github/workflows/release.yml), which:

1. Extracts the `## [0.2.0]` section from `CHANGELOG.md`
   (via [`scripts/extract-changelog.sh`](scripts/extract-changelog.sh)) and
   **creates the GitHub Release** with those exact notes.
2. Builds the **CLI/server binary** for Linux, macOS (x86-64 + arm64), and
   Windows, and attaches each as a `.tar.gz` / `.zip`.
3. Builds the **Tauri desktop app** for each platform and attaches the
   installers/bundles.

Every build in that workflow has `SLICER_VERSION` pinned to the tag, so the
artifacts report the correct version even on a shallow checkout.

## Verifying a release locally

```bash
# What version will this checkout report?
cargo run -- info

# What are the embedded notes?
cargo run -- changelog                 # full changelog
cargo run -- changelog --version 0.2.0 # one section
cargo run -- changelog --json          # machine-readable
```

On a clean checkout of the tag, `cargo run -- info` should print `0.2.0` with
channel `release`; anywhere else it prints `development`.

## Pre-releases

Tag with a suffix — `v0.2.0-rc.1` — and the workflow marks the GitHub Release as
a pre-release. Add a matching `## [0.2.0-rc.1]` section to `CHANGELOG.md` (or the
notes fall back to auto-generated).

## Canary builds

Stable releases are tag-driven and deliberate (above). For quick access to the
bleeding edge, [`.github/workflows/canary.yml`](.github/workflows/canary.yml)
fires on **every push to `main`** (i.e. every merge) and refreshes a single
rolling **`canary`** GitHub **pre-release** with fresh Windows and macOS desktop
bundles.

- **Not a real release.** The version is a throwaway pre-release string
  (`X.Y.Z-canary.<run>+<sha>`), so builds still report as unofficial and the UI
  never nags a "What's New" dialog for them.
- **No changelog needed.** The notes are just the commit range since the last
  `v*` tag — no `CHANGELOG.md` curation is involved.
- **Always the tip of `main`.** The rolling `canary` tag and its assets are
  overwritten on each push, so the Releases tab always offers the latest build.

To publish a *stable*, versioned release, follow the tag-driven flow above.

## macOS bundles & code signing

Both desktop workflows build a **universal** macOS binary
(`universal-apple-darwin`), so a single `.dmg` runs on Intel *and* Apple Silicon.

By default the app is only **ad-hoc signed** (`APPLE_SIGNING_IDENTITY=-`). That
is enough to launch it, but because it is not notarized, macOS attaches a
quarantine flag to the downloaded bundle and Gatekeeper reports the app as
**"damaged and can't be opened"**. Clearing the flag once fixes it:

```bash
xattr -cr "/Applications/Cold Crabby Desktop.app"
```

The canary and release notes already spell this out for users.

### Shipping notarized builds

To give users a clean double-click experience (no `xattr` dance), add these repo
**secrets** — both workflows detect them automatically and switch from ad-hoc
signing to real Developer ID signing + notarization:

| Secret                       | What it is                                          |
| ---------------------------- | --------------------------------------------------- |
| `APPLE_SIGNING_IDENTITY`     | e.g. `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_CERTIFICATE`          | base64 of the exported `.p12`                       |
| `APPLE_CERTIFICATE_PASSWORD` | password for that `.p12`                            |
| `APPLE_ID`                   | your Apple ID email                                 |
| `APPLE_PASSWORD`             | an app-specific password for notarization           |
| `APPLE_TEAM_ID`              | your 10-character Apple Team ID                      |

This requires a paid Apple Developer account. Until those are set, the ad-hoc +
`xattr` path above is the supported way to run the desktop app.

## See also

- [`release` skill](.github/skills/release/SKILL.md) — automates this process locally.
- [CHANGELOG.md](CHANGELOG.md) — the notes themselves.
- [`build.rs`](build.rs) — version derivation from git.
- [`src/version.rs`](src/version.rs) — the version/changelog API.
- [`.github/workflows/release.yml`](.github/workflows/release.yml) — the pipeline.
