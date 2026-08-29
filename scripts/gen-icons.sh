#!/usr/bin/env bash
#
# gen-icons.sh — regenerate every app icon from the official logo master.
#
# The master is `ui-desktop/src-tauri/app-icon.png`: a 1024x1024 opaque crop of
# `ui/public/logo_source.png` using the shipping app-icon framing. Regenerate it
# whenever the brand artwork changes, then commit the results.
#
# Usage:
#   scripts/gen-icons.sh
#
# Covers macOS/Windows/Linux (`ui-desktop/src-tauri/icons/`), iOS
# (`gen/apple/Assets.xcassets`) and Android, if those projects are generated.
set -euo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || echo .)"

readonly MASTER="src-tauri/app-icon.png"
readonly IOS_ICONS="ui-desktop/src-tauri/gen/apple/Assets.xcassets/AppIcon.appiconset"

if [[ ! -f "ui-desktop/$MASTER" ]]; then
  echo "error: icon master not found at ui-desktop/$MASTER" >&2
  exit 1
fi

echo "Generating icons from ui-desktop/${MASTER}…"
# `--ios-color` is the background the artwork is composited onto for iOS, which
# forbids transparency. White matches the logo's own backdrop.
(cd ui-desktop && pnpm exec tauri icon "$MASTER" --ios-color '#ffffff')

# `tauri icon` always emits every platform it knows about. We ship dmg/app/msi/
# nsis and iOS, none of which read the MSIX/UWP tiles or the Android mipmaps, so
# drop them rather than committing artwork nothing loads. Delete this pruning if
# Android is ever set up.
rm -rf ui-desktop/src-tauri/icons/android
rm -f ui-desktop/src-tauri/icons/Square*Logo.png ui-desktop/src-tauri/icons/StoreLogo.png

# `tauri icon` writes iOS icons as RGBA even when the source is opaque and the
# background colour is applied. App Store Connect rejects any icon carrying an
# alpha channel (ITMS-90717) regardless of whether it is fully opaque, so drop
# the channel. Alpha is uniformly 255 here, making this lossless.
if [[ -d "$IOS_ICONS" ]]; then
  echo "Flattening iOS icons to RGB (App Store forbids an alpha channel)…"
  python3 - "$IOS_ICONS" <<'PY'
import pathlib
import sys

from PIL import Image

flattened = 0
for path in sorted(pathlib.Path(sys.argv[1]).glob("*.png")):
    with Image.open(path) as image:
        if image.mode != "RGBA":
            continue
        alpha = image.split()[3]
        low, _ = alpha.getextrema()
        if low != 255:
            # Genuine transparency would change appearance if simply dropped;
            # composite onto white instead so nothing silently goes black.
            flat = Image.new("RGB", image.size, (255, 255, 255))
            flat.paste(image, mask=alpha)
        else:
            flat = image.convert("RGB")
    flat.save(path, format="PNG", optimize=True)
    flattened += 1

print(f"  flattened {flattened} icon(s)")
PY
fi

echo "Done. Review the diff, then commit the regenerated icons."
