#!/usr/bin/env bash
#
# gen-logo-assets.sh — regenerate every logo asset the app ships, from one master.
#
# Two surfaces draw the mascot, and both are derived here so neither can drift
# from `logo_still@3x.png`.
#
# **The boot splash** in `ui/src/index.html` paints before any stylesheet or
# bundle has arrived, so it shows the logo in two stages:
#
#   1. a ~700-byte placeholder embedded in the document as base64, which costs
#      no request at all and is therefore on screen with the first paint;
#   2. `ui/public/splash-logo.webp`, the full-resolution artwork, which fades in
#      over it once it arrives.
#
# Progressive JPEG — the usual answer for "rough now, sharp later" — cannot be
# used: the logo is RGBA and JPEG has no alpha channel. Neither WebP nor AVIF
# decodes progressively either, so the refinement is staged explicitly. That is
# also strictly faster than a progressive format, whose first pass still costs a
# round trip while an inlined placeholder costs none.
#
# **The in-app logo** (`ui/src/app/components/logo/`) sits in the header on every
# screen. It used to be served as PNG, where the 2x variant a phone actually
# picks weighed 52 KB for a 168px image — more than the rest of the page's
# images together. The same artwork as WebP is 14 KB.
#
# WebP carries no PNG fallback deliberately: it has been supported everywhere
# since Safari 14 (2020), and this app needs WebAssembly and WebGL2 to do
# anything at all, so no browser that can run it lacks WebP. The PNGs stay in
# `ui/public/` as the masters this script reads, not as anything the app serves.
#
# Requires `cwebp` (brew install webp).
#
# Usage:
#   scripts/gen-logo-assets.sh          # regenerate every asset
#   scripts/gen-logo-assets.sh --check  # verify they are current (for CI)
set -euo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || echo .)"

readonly SOURCE="ui/public/logo_still@3x.png"
readonly FULL_ASSET="ui/public/splash-logo.webp"
readonly INDEX="ui/src/index.html"

# Scan every argument rather than reading `$1`: `pnpm run logo-assets -- --check`
# forwards the `--` separator as the first argument, so a positional read sees
# `--` and silently takes the write path instead of checking.
MODE=""
for arg in "$@"; do
  case "$arg" in
    --check) MODE="--check" ;;
    --) ;;
    *)
      echo "error: unknown argument: $arg" >&2
      echo "usage: scripts/gen-logo-assets.sh [--check]" >&2
      exit 2
      ;;
  esac
done
readonly MODE

# The splash draws the logo at 120 CSS px, so 240 keeps it crisp on 2x displays.
readonly FULL_PX=240
readonly FULL_QUALITY=88
# Small enough to stay cheap inside a render-blocking document, large enough to
# read as the mascot once blurred and scaled up.
readonly PLACEHOLDER_PX=20
readonly PLACEHOLDER_QUALITY=60

if ! command -v cwebp >/dev/null 2>&1; then
  echo "error: cwebp not found — install it with 'brew install webp'" >&2
  exit 1
fi

if [[ ! -f "$SOURCE" ]]; then
  echo "error: logo master not found at $SOURCE" >&2
  exit 1
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Emit `$2` from the freshly built `$1`, or verify it matches under --check.
emit() {
  local built="$1" asset="$2"
  if [[ "$MODE" == "--check" ]]; then
    if ! cmp -s "$built" "$asset"; then
      echo "error: $asset is stale — run scripts/gen-logo-assets.sh" >&2
      exit 1
    fi
    echo "$asset is up to date"
  else
    cp "$built" "$asset"
    echo "wrote $asset ($(wc -c <"$asset" | tr -d ' ') bytes)"
  fi
}

# Full-resolution asset.
sips -Z "$FULL_PX" "$SOURCE" --out "$tmp/full.png" >/dev/null
cwebp -quiet -q "$FULL_QUALITY" "$tmp/full.png" -o "$tmp/full.webp"
emit "$tmp/full.webp" "$FULL_ASSET"

# In-app logo. One file per `srcset` descriptor, so a phone downloads the 2x it
# needs and nothing larger. Re-encoded from the PNG of the same size rather than
# resampled from the master, so each stays pixel-identical to the artwork it
# replaces.
for scale in "" "@2x" "@3x"; do
  png="ui/public/logo_still${scale}.png"
  if [[ ! -f "$png" ]]; then
    echo "error: logo master not found at $png" >&2
    exit 1
  fi
  cwebp -quiet -q "$FULL_QUALITY" "$png" -o "$tmp/logo${scale}.webp"
  emit "$tmp/logo${scale}.webp" "ui/public/logo_still${scale}.webp"
done

# Inline placeholder.
sips -Z "$PLACEHOLDER_PX" "$SOURCE" --out "$tmp/lqip.png" >/dev/null
cwebp -quiet -q "$PLACEHOLDER_QUALITY" -alpha_q 60 "$tmp/lqip.png" -o "$tmp/lqip.webp"
data="data:image/webp;base64,$(base64 -i "$tmp/lqip.webp" | tr -d '\n')"

python3 - "$INDEX" "$data" "$MODE" <<'PY'
import pathlib
import re
import sys

target, data, mode = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
html = target.read_text()

# The blob lives behind one custom property so nothing else knows its shape.
# Either quote style is accepted and single quotes are written back, because
# Prettier formats this file and normalises CSS urls to single quotes — matching
# only double quotes made this script silently stop finding its own output.
pattern = re.compile(r"""(--boot-logo-placeholder:\s*url\()['"][^'"]*['"](\))""")
if not pattern.search(html):
    sys.exit(f"error: placeholder declaration not found in {target}")

updated = pattern.sub(lambda m: f"{m.group(1)}'{data}'{m.group(2)}", html, count=1)

if mode == "--check":
    if updated != html:
        sys.exit(f"error: {target} placeholder is stale — run scripts/gen-splash-logo.sh")
    print(f"{target} placeholder is up to date")
elif updated == html:
    print(f"{target} placeholder already current")
else:
    target.write_text(updated)
    print(f"{target} placeholder updated ({len(data)} bytes inline)")
PY
