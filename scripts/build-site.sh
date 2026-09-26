#!/usr/bin/env bash
#
# build-site.sh — build the static site: the browser-only web slicer at the
# root and the docs under /docs/.
#
# One build, two destinations: the GitHub Pages deploy of main and the
# per-pull-request preview both call this, so a preview is exactly what main
# would publish — only the host differs. Host-specific touches (a 404 page for
# SPA fallback, .nojekyll) are the caller's job.
#
# Usage:
#   scripts/build-site.sh            # into _site/
#   scripts/build-site.sh out/       # into out/
#
# Needs the toolchains the web-slicer build needs (Rust with the wasm32 target,
# wasm-bindgen-cli, the WASI SDK on PATH, pnpm dependencies installed).
# SLICER_GIT_SHA may be set to pin the SHA baked into the bundle; version.json
# carries the same value so a stale tab can detect a newer deploy.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

out="${1:-_site}"
sha="${SLICER_GIT_SHA:-$(git rev-parse HEAD)}"
ref="${SLICER_DEPLOY_REF:-$(git rev-parse --abbrev-ref HEAD)}"

export SLICER_GIT_SHA="${sha}"
export CXX_wasm32_unknown_unknown="${CXX_wasm32_unknown_unknown:-${PWD}/tools/wasm32-clang++.js}"

pnpm run hydrate:web-slicer
pnpm --filter slicer-ui exec ng build --configuration web-slicer --base-href /
pnpm --filter slicer-engine-docs docs:build

rm -rf "${out}"
mkdir -p "${out}/docs"

if [[ -d ui/dist/slicer-ui/browser ]]; then
  cp -R ui/dist/slicer-ui/browser/. "${out}/"
elif [[ -d ui/dist/slicer-ui ]]; then
  cp -R ui/dist/slicer-ui/. "${out}/"
else
  echo "Could not find Angular build output under ui/dist/slicer-ui" >&2
  exit 1
fi
if [[ ! -f "${out}/index.html" ]]; then
  echo "Expected ${out}/index.html in the Angular build output" >&2
  exit 1
fi

cp -R docs/.vitepress/dist/. "${out}/docs/"

# Deploy manifest for the UI's out-of-date detector. Its SHA matches the one
# baked into the WASM bundle; a stale tab re-fetches this file and prompts a
# reload when they differ.
printf '{"sha":"%s","version":"%s"}\n' "${sha}" "${ref}" > "${out}/version.json"

echo "Site built into ${out}/"
