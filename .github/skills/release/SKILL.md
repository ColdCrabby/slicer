---
name: release
description: Cut a release locally — curate CHANGELOG.md, acknowledge contributors (spotlighting first-timers), tag, and push to trigger the GitHub Release. Use when the user says "release", "cut a release", "prepare a release", "ship a version", "write release notes", or "bump the version".
---

# Cut a Release

Drive the whole local release flow for Slicer Engine: turn the commits since the
last tag into curated, enthusiastic release notes, acknowledge every
contributor (and give first-timers an extra spotlight), then tag and push so
[`.github/workflows/release.yml`](../../workflows/release.yml) builds and
publishes the GitHub Release.

**A git tag is the single source of truth.** You are producing two artifacts —
a curated `CHANGELOG.md` section and a `vX.Y.Z` tag. Everything else (baked-in
version, GitHub Release notes, the UI "What's New" dialog) is derived from those.
See [RELEASING.md](../../../RELEASING.md) for the surrounding system.

## Guardrails

- **Never push or tag without explicit user confirmation.** Show the final notes
  and the version, and wait for a "yes" before `git tag` / `git push`.
- **Never invent changes.** Every bullet must trace to a real commit in the range.
- **Never fabricate contributors.** Use only what the scripts report from git.
- **Don't skip hooks or force anything.** No `--no-verify`, no `--force`.
- If the working tree is dirty or the branch isn't the release branch, stop and
  ask before proceeding.

## Procedure

### 1. Preflight

```bash
git rev-parse --abbrev-ref HEAD          # expect the release branch (usually main)
git status --short                        # expect clean
git describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || echo "(no prior tag)"
```

If the tree is dirty, ask the user to commit/stash first. Confirm the branch is
the one they intend to release from.

### 2. Gather the facts (never guess)

```bash
scripts/gen-changelog-draft.sh           # categorised commit draft since last tag
scripts/release-contributors.sh          # contributors + first-timers since last tag
```

Read the actual commits too when a subject is terse:

```bash
git log --no-merges --format='%h %s' <last-tag>..HEAD
```

### 3. Decide the version

Infer a [SemVer](https://semver.org/) bump from the commits and **confirm with
the user**:

| Signal in the range                          | Bump   |
| -------------------------------------------- | ------ |
| Any breaking change (`feat!:`, `BREAKING`)   | major  |
| Any `feat:`                                  | minor  |
| Only `fix:` / `perf:` / `docs:` / chores     | patch  |

Recommend a version, state your reasoning in one line, and let the user override.

### 4. Curate the CHANGELOG section — the voice matters

Edit [`CHANGELOG.md`](../../../CHANGELOG.md). Rewrite the `## [Unreleased]`
content into a polished, dated section. **Do not just paste the draft** — the
draft is raw material; you are writing for humans (this exact text ships to
users in the "What's New" dialog and becomes the GitHub Release body).

Structure each release section like this:

```markdown
## [0.2.0] - 2026-09-01

One or two sentences that lead with the single biggest thing in this release
and why anyone should care. Tight, concrete, energetic — this is the headline.

### Highlights

- **The marquee feature** — what it unlocks, in one vivid line. Lead with the
  outcome, not the implementation.
- **The second-biggest feature** — same treatment, if there is one.

### Added
- Concrete, user-facing additions. One line each.

### Changed
- Behaviour changes and improvements.

### Fixed
- Notable fixes. Skip trivial internal churn.

### Contributors

Thanks to everyone who shipped this release: @alice, @bob, @carol.

A special welcome to our first-time contributors — @carol landed their first
change here. Thank you, and welcome aboard.
```

**Tone rules (follow precisely):**

- **Broad but key-facts-tight.** Cover the release at a glance; every sentence
  earns its place. No filler, no marketing fog, no restating the obvious.
- **Genuinely enthusiastic.** Write like you're proud of the work. Lead with
  what the reader gains. Verbs over adjectives.
- **Minimal emojis.** At most one, and only if it genuinely amplifies the
  energy. A wall of emojis *undermines* the excitement — restraint reads as
  confidence. Prefer strong words to symbols.
- **Biggest features first.** The "Highlights" (and the opening line) spotlight
  the one or two changes that define this release. Everything else is supporting
  detail under Added/Changed/Fixed.
- **Facts, not hype.** Enthusiasm rides on real capability. If a claim isn't
  backed by a commit, cut it.

**Contributor acknowledgement (make this shine):**

- List **every** contributor the script reports. Use GitHub handles where the
  email maps to one (`name@users.noreply.github.com` → the name is the handle;
  `NNN+login@users.noreply.github.com` → `@login`). When unsure, use the display
  name.
- **Exclude bot identities** from thanks (e.g. `Copilot`,
  `copilot-swe-agent[bot]`) — acknowledge human contributors. If a human
  co-authored with a bot, thank the human.
- **Spotlight first-time contributors loudly.** Anyone under `NEW CONTRIBUTORS`
  gets a warm, explicit welcome by name. This is the emotional peak of the notes
  — be generous and specific ("landed their first change", "jumped straight into
  the hardest part of the pipeline"). New contributors are the lifeblood of the
  project; make them feel it.
- For the **first ever release** (no prior tag), the script flags everyone as
  new — don't call every author a "first-timer" in that case; instead thank the
  founding contributors warmly.

After rewriting the dated section, add a fresh empty `## [Unreleased]` heading
above it so the next cycle has a home.

### 5. Review with the user

Show the rendered section and the chosen version. Get explicit approval. Revise
until they're happy. **Do not proceed to tagging without a clear yes.**

### 6. Commit, tag, push

Use the repository's Conventional Commits style (see the `commit` skill for
message conventions).

```bash
git add CHANGELOG.md
git commit -m "docs: changelog for <version>"
git tag "v<version>"
```

Confirm once more, then:

```bash
git push origin <branch> --follow-tags
```

Pushing the tag triggers the release workflow. Point the user at the Actions run
and the eventual Release page.

### 7. Verify

```bash
scripts/extract-changelog.sh <version>   # exactly what the Release body will be
```

Confirm it matches the curated section. If a clean checkout of the tag is
available, `cargo run -- info` should report `<version>` on the `release`
channel.

## Example — turning raw material into notes

**Raw draft (from `gen-changelog-draft.sh`):**

```
### Added
- implement Arachne medial-axis wall generator
- enhance Arachne wall generation with variable-width support and gap-fill parameters
- add tools for analyzing extrusion width and detecting wall-zone gaps
```

**Contributors (from `release-contributors.sh`):**

```
CONTRIBUTORS: Max Scopp <me@maxscopp.de>, Jane Dev <jane@users.noreply.github.com>
NEW CONTRIBUTORS: Jane Dev <jane@users.noreply.github.com>
```

**Curated section:**

```markdown
## [0.2.0] - 2026-09-01

Walls just got dramatically smarter. This release lands Arachne — a
medial-axis wall generator that varies bead width to fill thin features
classic uniform walls leave hollow.

### Highlights

- **Arachne variable-width walls** — the pipeline now grows beads that widen and
  narrow to follow the model, closing the gaps thin geometry used to leave behind.

### Added
- Variable-width wall generation with gap-fill parameters.
- G-code analysis tools for extrusion width and wall-zone void detection.

### Contributors

Thanks to everyone who shipped this release: @max-scopp, @jane.

And a warm welcome to @jane — this is their first contribution, straight into
the heart of the wall generator. Fantastic start, and thank you.
```

Notice: one headline sentence, highlights lead with outcome, no emoji pile-up,
and the new contributor gets a real, specific spotlight.
