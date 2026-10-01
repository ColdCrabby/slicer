#!/usr/bin/env bash
#
# full-changelog-link.sh — the "Full changelog" line at the top of a release.
#
# CHANGELOG.md holds the curated, user-facing story. Anyone who wants every
# commit underneath it gets one link to GitHub's compare view, from the previous
# *stable* release to this one — not a list pasted into the notes.
#
# Usage:
#   scripts/full-changelog-link.sh               # previous stable tag...HEAD
#   scripts/full-changelog-link.sh v0.6.0        # previous stable tag...v0.6.0
#   scripts/full-changelog-link.sh v0.6.0 v0.5.0 # explicit range
#
# "Previous stable" skips release candidates, so an RC and its final release
# link the same range: everything the release contains.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

to="${1:-HEAD}"
from="${2:-}"
if [[ -z "${from}" ]]; then
  # The newest stable tag strictly before `to` (so a tag never links against itself).
  from="$(git describe --tags --abbrev=0 --match 'v[0-9]*' --exclude 'v*-*' "${to}^" 2>/dev/null || true)"
fi

# A tag links by name; anything else (HEAD, a branch) by the commit it is now.
if git show-ref --verify --quiet "refs/tags/${to}"; then
  ref="${to}"
  label="${to}"
else
  ref="$(git rev-parse "${to}")"
  label="${ref::7}"
fi

if [[ -n "${GITHUB_SERVER_URL:-}" && -n "${GITHUB_REPOSITORY:-}" ]]; then
  repo="${GITHUB_SERVER_URL}/${GITHUB_REPOSITORY}"
else
  repo="$(git remote get-url origin | sed -E 's#^git@([^:]+):#https://\1/#; s#\.git$##')"
fi

if [[ -n "${from}" ]]; then
  echo "**Full changelog**: [${from}...${label}](${repo}/compare/${from}...${ref})"
else
  echo "**Full changelog**: [every commit to ${label}](${repo}/commits/${ref})"
fi
