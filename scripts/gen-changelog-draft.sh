#!/usr/bin/env bash
#
# gen-changelog-draft.sh — draft an "Unreleased" changelog section from git log.
#
# This is step one of the *hybrid* changelog workflow: it turns the commits
# since the last release tag into a categorised markdown draft that you then
# hand-edit into CHANGELOG.md before tagging. It never writes any file.
#
# Usage:
#   scripts/gen-changelog-draft.sh            # commits since the last v* tag
#   scripts/gen-changelog-draft.sh v0.2.0     # commits since an explicit tag
#
# Commits are grouped by their Conventional Commit prefix (feat:, fix:, …).
# Anything without a recognised prefix lands under "Other".
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

since_tag="${1:-}"
if [[ -z "${since_tag}" ]]; then
  since_tag="$(git describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || true)"
fi

if [[ -n "${since_tag}" ]]; then
  range="${since_tag}..HEAD"
  echo "# Draft changelog for commits since ${since_tag}" >&2
else
  range="HEAD"
  echo "# Draft changelog (no prior tag found — using full history)" >&2
fi

added=()
fixed=()
changed=()
docs=()
other=()

while IFS= read -r subject; do
  [[ -z "${subject}" ]] && continue
  case "${subject}" in
    feat:*|feat\(*) added+=("${subject#*: }") ;;
    fix:*|fix\(*) fixed+=("${subject#*: }") ;;
    perf:*|perf\(*|refactor:*|refactor\(*|change:*|chore:*|chore\(*) changed+=("${subject#*: }") ;;
    docs:*|docs\(*) docs+=("${subject#*: }") ;;
    *) other+=("${subject}") ;;
  esac
done < <(git log --no-merges --format='%s' "${range}")

print_section() {
  local heading="$1"; shift
  local -a items=("$@")
  [[ ${#items[@]} -eq 0 ]] && return
  echo "### ${heading}"
  echo
  for item in "${items[@]}"; do
    echo "- ${item}"
  done
  echo
}

echo "## [Unreleased]"
echo
print_section "Added" "${added[@]+"${added[@]}"}"
print_section "Changed" "${changed[@]+"${changed[@]}"}"
print_section "Fixed" "${fixed[@]+"${fixed[@]}"}"
print_section "Documentation" "${docs[@]+"${docs[@]}"}"
print_section "Other" "${other[@]+"${other[@]}"}"
