#!/usr/bin/env bash
#
# release-commits.sh — the full commit list for a release, for the nerds.
#
# CHANGELOG.md holds the curated, user-facing story. This prints the raw
# material underneath it: every commit since the previous *stable* release, as
# a collapsed <details> block the release workflows append to the GitHub
# Release body. It is deliberately not written into CHANGELOG.md — that file
# ships inside the app, where a wall of commit subjects helps nobody.
#
# Usage:
#   scripts/release-commits.sh               # previous stable tag..HEAD
#   scripts/release-commits.sh v0.6.0        # previous stable tag..v0.6.0
#   scripts/release-commits.sh v0.6.0 v0.5.0 # explicit range
#
# "Previous stable" skips release candidates, so an RC and its final release
# list the same range: everything the release contains. Trailing "(#123)"
# references are dropped — each line links its commit instead.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

to="${1:-HEAD}"
from="${2:-}"
if [[ -z "${from}" ]]; then
  # The newest stable tag strictly before `to` (so a tag never lists against itself).
  from="$(git describe --tags --abbrev=0 --match 'v[0-9]*' --exclude 'v*-*' "${to}^" 2>/dev/null || true)"
fi

if [[ -n "${from}" ]]; then
  range="${from}..${to}"
  summary="Every commit since ${from}"
else
  range="${to}"
  summary="Every commit"
fi

# Link each hash when running on GitHub Actions; elsewhere print it bare.
commit_url=""
if [[ -n "${GITHUB_SERVER_URL:-}" && -n "${GITHUB_REPOSITORY:-}" ]]; then
  commit_url="${GITHUB_SERVER_URL}/${GITHUB_REPOSITORY}/commit"
fi

lines=()
while IFS=$'\t' read -r sha short subject; do
  [[ -z "${sha}" ]] && continue
  subject="$(sed -E 's/[[:space:]]*\(#[0-9]+\)$//' <<<"${subject}")"
  if [[ -n "${commit_url}" ]]; then
    lines+=("- ${subject} ([\`${short}\`](${commit_url}/${sha}))")
  else
    lines+=("- ${subject} (\`${short}\`)")
  fi
done < <(git log --no-merges --format='%H%x09%h%x09%s' "${range}")

if [[ ${#lines[@]} -eq 0 ]]; then
  exit 0
fi

echo "<details>"
echo "<summary>${summary} (${#lines[@]})</summary>"
echo
printf '%s\n' "${lines[@]}"
echo
echo "</details>"
