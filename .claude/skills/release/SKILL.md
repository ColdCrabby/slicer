---
name: release
description: Prepare and ship a release — freshen up the Unreleased changelog notes, acknowledge contributors (spotlighting first-timers), cut a release candidate for a final test sweep, then tag the real release. Use when the user says "release", "cut a release", "prepare a release", "release candidate", "RC", "ship it", "ship a version", "write release notes", or "bump the version".
---

# Prepare and Ship a Release

A release goes out in two passes, with a test sweep between them:

1. **Prepare** — turn the commits since the last release into short, enjoyable
   notes, acknowledge every contributor, and tag a **release candidate**
   (`vX.Y.Z-rc.N`). CI builds it exactly as the release will be built.
2. **Ship** — once the candidate has been tested (and maybe got a tiny fix),
   date the notes and tag **`vX.Y.Z`** on the tested commit.

Between releases, every merge already publishes a **Latest dev build** on its
own — nothing in this skill touches it.

**A git tag is the single source of truth.** You produce a curated
`CHANGELOG.md` section and two tags. Everything else — the baked-in version, the
GitHub Release body, the in-app "What's New" — is derived from those. The full
commit list is added to the GitHub Release automatically; never paste it into
`CHANGELOG.md`. See [RELEASING.md](../../../RELEASING.md) for the whole system.

## Guardrails

- **Never push or tag without explicit user confirmation** — once for the
  candidate, again for the release. Show the notes and the version, and wait
  for a "yes" before each `git tag` / `git push`.
- **Never skip the candidate.** Even a small release gets an `-rc.1` and a test
  sweep; that is the point of the flow.
- **Never invent changes.** Every bullet must trace to a real commit in the range.
- **Never fabricate contributors.** Use only what the scripts report from git.
- **Don't skip hooks or force anything.** No `--no-verify`, no `--force`.
- If the working tree is dirty or the branch isn't the release branch, stop and
  ask before proceeding.

## Pass 1 — Prepare the release candidate

### 1. Preflight

```bash
git rev-parse --abbrev-ref HEAD          # expect main
git status --short                        # expect clean
git describe --tags --abbrev=0 --match 'v[0-9]*' --exclude 'v*-*' 2>/dev/null || echo "(no prior release)"
git tag --list 'v*-rc.*' --sort=-v:refname | head -3   # any candidate in flight?
```

If the tree is dirty, ask the user to commit or stash first. If a candidate for
the next version already exists, you are in **Pass 2** (or cutting `rc.N+1`) —
skip ahead.

### 2. Gather the facts (never guess)

```bash
scripts/gen-changelog-draft.sh           # categorised commit draft since last tag
scripts/release-contributors.sh          # contributors + first-timers since last tag
scripts/release-commits.sh               # the commit list GitHub will show
```

Read the actual commits too when a subject is terse:

```bash
git log --no-merges --format='%h %s' <last-tag>..HEAD
```

### 3. Decide the version

Infer a [SemVer](https://semver.org/) bump from the commits since the last
**stable** tag and **confirm with the user**:

| Signal in the range                          | Bump   |
| -------------------------------------------- | ------ |
| Any breaking change (`feat!:`, `BREAKING`)   | major  |
| Any `feat:`                                  | minor  |
| Only `fix:` / `perf:` / `docs:` / chores     | patch  |

Recommend a version, state your reasoning in one line, and let the user override.

### 4. Freshen up the notes — the voice matters

Edit [`CHANGELOG.md`](../../../CHANGELOG.md). **Always rewrite the
`## [Unreleased]` section — never promote it as it stands.** Entries are added
one change at a time, by whoever landed it, and they read that way: too long,
too technical, repetitive. Turn them into notes someone is glad to read — this
exact text is the "What's New" dialog in the app and the GitHub Release body.

Write for a person who prints things, not for a person who reads the code:

- **Short.** A reader should get the whole release in under a minute. Merge
  related entries, drop the ones nobody would notice, cut every sentence that
  explains *how* instead of *what you get*.
- **Enticing.** Lead with what they can do now. Make them want to update.
- **Useful.** Say where to find it or what to press when that is not obvious.
  One number is fine when it is the point ("fits a third more parts on a
  plate"); a measurement from a debugging session is not.
- **No technicalities.** No algorithm names unless users already know them
  (Arachne, yes; Voronoi, no), no hash orders, no internal type or file names,
  no pipeline stages. Those live in the module READMEs and the commit list.
- **Keep the draft as raw material only.** `gen-changelog-draft.sh` and the
  existing `Unreleased` entries tell you what happened; the notes you write say
  why it matters.

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
- For a large release, break the list into `#### Theme` groups (see the tone
  rules below) rather than one long undifferentiated run.

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
- **Condensed, not exhaustive.** Each entry is one or two tight lines — what it
  does for the reader, and where to find it if that isn't obvious. Deep
  rationale (why an algorithm works, measured bead deltas, pipeline ordering)
  belongs in `AGENTS.md` and the module READMEs, **not** here. If a bullet grows
  into a paragraph of justification, you're writing the wrong document.
- **The nerds are already covered.** Every commit since the last release is
  appended to the GitHub Release in a folded list, automatically. That is what
  frees these notes to leave the small stuff out — never paste it in by hand.
- **Group a long category under `####` subheadings by theme.** One flat run of 25
  bullets is unscannable; a handful of themed groups (e.g. *Infill & surfaces*,
  *Multi-object build plates*, *Printer & firmware output*, *App, platform &
  tooling*) is a map. Keep the bold `**Feature name**` lead on every bullet.
- **No issue or PR numbers, and no repo links, in the notes.** Describe the
  change, not its tracking ticket — `#123`, `(#123)`, `GH-123`, or a full
  issue URL are all noise to a reader and rot when the repo moves. (A `Closes
  #NN` line belongs in a PR description, never in the changelog.)
- **Write for how it renders in-app.** The "What's New" view styles both `###`
  and `####` as small uppercase labels, so the prominent text is the **bold
  bullet lead**, not a heading. Name features in bold; use headings only as
  category and group labels.

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

Rename the rewritten section to `## [X.Y.Z] - <today>` and add a fresh empty
`## [Unreleased]` heading above it so the next cycle has a home.

### 5. Review with the user

Show the rendered section and the chosen version. Revise until they're happy.
**Do not tag without a clear yes.**

### 6. Commit and tag the candidate

The section is headed `## [X.Y.Z] - <today>` — the candidate has no heading of
its own; it shows the `X.Y.Z` notes under a "release candidate" banner. Use the
repository's Conventional Commits style.

```bash
git add CHANGELOG.md
git commit -m "docs: changelog for <version>"
git tag "v<version>-rc.1"
git push origin main "v<version>-rc.1"
```

Pushing the tag triggers the release workflow, which publishes a GitHub
**pre-release** with every platform's build. Point the user at it, then hand
over the test sweep: offer the [`test-changes`](../test-changes/SKILL.md)
checklist for what the notes promise.

## Between the passes — the test sweep

The user installs the candidate and tries it. What comes back decides the next
step:

| Found | Do |
| --- | --- |
| Nothing | Pass 2. |
| A tiny, obviously safe fix | Land it on `main` (its own PR, as usual), then Pass 2. |
| Anything that needs testing again | Land the fix, then tag `v<version>-rc.2` and sweep again. |

A user-visible fix gets a line in the `## [<version>]` section — not in
`Unreleased`, which is already collecting the *next* release.

## Pass 2 — Ship the release

### 1. Check what you are about to tag

```bash
git log --oneline "v<version>-rc.<n>"..main
```

That range must be only the fixes from the sweep. If unrelated work has merged
since the candidate, **do not tag `main`** — it would ship untested changes.
Create `release/<major>.<minor>` from the candidate's tag, cherry-pick the fixes
onto it, and do the steps below there. Ask the user when in doubt.

### 2. Date the notes, confirm, tag

Set the section's date to today if it has moved and make sure any sweep fix is
in it. Show the final section once more and get a yes.

```bash
git add CHANGELOG.md
git commit -m "docs: date the <version> release"   # only if something changed
git tag "v<version>"
git push origin <branch> "v<version>"
```

### 3. Verify

```bash
scripts/extract-changelog.sh <version>   # exactly what the Release body starts with
```

Confirm it matches the curated section. On a clean checkout of the tag,
`cargo run -- info` should report `<version>` on the `release` channel. Point
the user at the Actions run and the Release page.

## Example — freshening up the notes

**Raw material** — an `Unreleased` entry as it was written when the fix landed,
plus a line from `gen-changelog-draft.sh`:

```markdown
- **The same model now slices to the same G-code.** Simplifying a region before
  Arachne's medial fill could leave two of its edges crossing, and the Voronoi
  diagram of crossing edges is undefined — so two runs of one file could print
  different walls there. Regions are now made clean again after simplifying, and
  the surface trim no longer depends on hash order. Classic was never affected.
```

```
### Added
- add an object library: every model once, with a picture, on every platform
```

**Contributors (from `release-contributors.sh`):**

```
CONTRIBUTORS: Max Scopp <me@maxscopp.de>, Jane Dev <jane@users.noreply.github.com>
NEW CONTRIBUTORS: Jane Dev <jane@users.noreply.github.com>
```

**Freshened section:**

```markdown
## [0.6.0] - 2026-10-01

Every model you have ever printed, one click away. The new Library keeps each
model once, with a picture, so the next plate starts from what you already have.

### Highlights

- **Library** — open it beside the plate and pick a model to drop it straight
  on. Renamed copies and re-exports are recognised as the same model.

### Fixed
- **Slicing is repeatable.** The same file now always gives the same G-code;
  thin walls could come out slightly different from one slice to the next.

### Contributors

Thanks to everyone who shipped this release: @max-scopp, @jane.

And a warm welcome to @jane — this is their first contribution, straight into
the new Library. Fantastic start, and thank you.
```

Notice: the fix went from five lines of mechanism to one line of what the user
gets; the headline says why to update; the new contributor gets a real, specific
spotlight. The Voronoi detail isn't lost — it is in the commit, and the commit
is in the folded list on the GitHub Release.
