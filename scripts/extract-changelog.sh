#!/usr/bin/env bash
#
# extract-changelog.sh — print the CHANGELOG.md body for a single version.
#
# Used by the release workflow to turn a tag into GitHub Release notes, so the
# release body always matches CHANGELOG.md (the single source of truth).
#
# Usage:
#   scripts/extract-changelog.sh 1.2.0      # prints the "## [1.2.0]" body
#   scripts/extract-changelog.sh v1.2.0     # leading "v" is stripped
#
# Exits non-zero if the version has no section.
set -euo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || echo .)"

version="${1:?usage: extract-changelog.sh <version>}"
version="${version#v}"

body="$(awk -v target="${version}" '
  /^## \[/ {
    label = $0
    sub(/^## \[/, "", label)
    sub(/\].*/, "", label)
    if (label == target) { capture = 1; next }
    else if (capture) { exit }
  }
  capture {
    # Buffer blank lines so leading/trailing blanks are trimmed on output.
    if ($0 ~ /^[[:space:]]*$/) { pending = pending "\n"; next }
    if (started) printf "%s", pending
    started = 1
    pending = ""
    print
  }
' CHANGELOG.md)"

if [[ -z "${body}" ]]; then
  echo "error: no changelog section for version '${version}'" >&2
  exit 1
fi

printf '%s\n' "${body}"
