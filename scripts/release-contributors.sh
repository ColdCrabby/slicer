#!/usr/bin/env bash
#
# release-contributors.sh — list contributors for a release range and flag
# first-time contributors, so release notes can acknowledge everyone and give
# newcomers an extra spotlight.
#
# Usage:
#   scripts/release-contributors.sh            # since the last v* tag
#   scripts/release-contributors.sh v0.2.0     # since an explicit tag
#
# Output is two labelled lists on stdout:
#   CONTRIBUTORS   — everyone (authors + co-authors) who landed in the range
#   NEW            — those with no commit anywhere before the range
#
# "New" is computed by comparing against all history reachable from the base
# tag. Bots (e.g. Copilot) are listed too — use judgement when acknowledging.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

since_tag="${1:-}"
if [[ -z "${since_tag}" ]]; then
  since_tag="$(git describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || true)"
fi

if [[ -n "${since_tag}" ]]; then
  range="${since_tag}..HEAD"
  base="${since_tag}"
else
  range="HEAD"
  base=""
fi

# Collect "Name <email>" for authors and Co-authored-by trailers in a range.
collect() {
  local rev_range="$1"
  {
    git log --no-merges --format='%aN <%aE>' "${rev_range}" 2>/dev/null || true
    git log --no-merges --format='%(trailers:key=Co-authored-by,valueonly)' "${rev_range}" 2>/dev/null \
      | sed '/^$/d'
  } | sed 's/^[[:space:]]*//; s/[[:space:]]*$//' | sort -u
}

range_people="$(collect "${range}")"

if [[ -n "${base}" ]]; then
  prior_people="$(collect "${base}")"
else
  prior_people=""
fi

# Match on email (the stable identity) to decide who is new.
email_of() { sed -n 's/.*<\(.*\)>.*/\1/p' <<<"$1"; }

prior_emails="$(while IFS= read -r p; do [[ -z "$p" ]] && continue; email_of "$p"; done <<<"${prior_people}" | sort -u)"

echo "CONTRIBUTORS (${range}):"
if [[ -z "${range_people//[$'\n\t ']/}" ]]; then
  echo "  (none)"
else
  while IFS= read -r p; do
    [[ -z "$p" ]] && continue
    echo "  ${p}"
  done <<<"${range_people}"
fi

echo
echo "NEW CONTRIBUTORS:"
found_new=0
while IFS= read -r p; do
  [[ -z "$p" ]] && continue
  email="$(email_of "$p")"
  if [[ -z "${base}" ]] || ! grep -qxF "${email}" <<<"${prior_emails}"; then
    echo "  ${p}"
    found_new=1
  fi
done <<<"${range_people}"
[[ "${found_new}" -eq 0 ]] && echo "  (none)"
