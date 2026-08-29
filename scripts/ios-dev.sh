#!/usr/bin/env bash
#
# ios-dev.sh — run the app on an iPad simulator with live reload.
#
# Wraps `tauri ios dev`, which otherwise looks for a physical device and then
# drops into an interactive picker dominated by iPhones. Resolving an iPad up
# front (see ios-simulator.sh) makes `pnpm run ios:dev` a single, no-prompt
# command for the form factor we actually target.
#
# Usage:
#   scripts/ios-dev.sh                            # default iPad simulator
#   scripts/ios-dev.sh --open                     # open Xcode instead of running
#   IOS_DEVICE='iPad mini (A17 Pro)' scripts/ios-dev.sh
#
# Any extra arguments are forwarded to `tauri ios dev`.
set -euo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || echo .)"

# `tauri ios dev` shells out to `xcodebuild` with a sanitized environment, so —
# unlike our other scripts — it cannot be steered with `DEVELOPER_DIR`. If the
# Command Line Tools are still selected it fails deep inside the build with a
# bare "requires Xcode" message, so check up front and say what to run.
if [[ "$(xcode-select -p 2>/dev/null)" == *CommandLineTools* ]]; then
  if [[ -d /Applications/Xcode.app/Contents/Developer ]]; then
    echo "error: Xcode is installed but the Command Line Tools are still selected." >&2
    echo "       'tauri ios dev' ignores DEVELOPER_DIR, so this must be fixed globally:" >&2
    echo >&2
    echo "         sudo xcode-select -s /Applications/Xcode.app" >&2
    echo >&2
  else
    echo "error: the full Xcode app is required (Command Line Tools have no iOS SDK)." >&2
    echo "       Run scripts/ios-doctor.sh for the full diagnosis." >&2
  fi
  exit 1
fi

if [[ ! -d ui-desktop/src-tauri/gen/apple ]]; then
  echo "error: the iOS Xcode project has not been generated yet." >&2
  echo "       Run: pnpm run ios:init" >&2
  exit 1
fi

device="$(scripts/ios-simulator.sh --name)"
echo "Target: $device"

# Boot it first so the Tauri CLI attaches to a ready simulator instead of
# racing the first-boot springboard, which often looks like a hung build.
scripts/ios-simulator.sh "$device" >/dev/null

exec pnpm --filter slicer-ui-desktop exec tauri ios dev "$device" "$@"
