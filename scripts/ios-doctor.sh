#!/usr/bin/env bash
#
# ios-doctor.sh — verify (and optionally repair) the toolchain needed to build
# and run Cold Crabby on the iOS/iPadOS Simulator or a physical iPad.
#
# Building a Tauri app for iOS needs more than "Rust + Xcode": the iOS SDK only
# ships with the full Xcode app (not Command Line Tools), Rust needs the two
# Apple mobile targets, and Tauri's generated Xcode project is driven by
# CocoaPods. Each of those fails with a different, unhelpful error message deep
# inside `xcodebuild`, so check them up front and say exactly what to run.
#
# Usage:
#   scripts/ios-doctor.sh          # report only
#   scripts/ios-doctor.sh --fix    # additionally install what can be automated
#
# Exits non-zero if any required check fails.
set -uo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || echo .)"

FIX=0
[[ "${1:-}" == "--fix" ]] && FIX=1

# Minimum iOS deployment target — keep in sync with
# ui-desktop/src-tauri/tauri.ios.conf.json > bundle.iOS.minimumSystemVersion.
readonly MIN_IOS="16.0"

# Where the full Xcode app is expected to live.
readonly XCODE_APP="${XCODE_APP:-/Applications/Xcode.app}"

if [[ -t 1 ]]; then
  BOLD=$'\033[1m'; RED=$'\033[31m'; GREEN=$'\033[32m'; YELLOW=$'\033[33m'; DIM=$'\033[2m'; RESET=$'\033[0m'
else
  BOLD=''; RED=''; GREEN=''; YELLOW=''; DIM=''; RESET=''
fi

failures=0
warnings=0

ok()   { printf '  %s✓%s %s\n' "$GREEN" "$RESET" "$1"; }
warn() { printf '  %s!%s %s\n' "$YELLOW" "$RESET" "$1"; warnings=$((warnings + 1)); }
bad()  { printf '  %s✗%s %s\n' "$RED" "$RESET" "$1"; failures=$((failures + 1)); }
hint() { printf '      %s→ %s%s\n' "$DIM" "$1" "$RESET"; }
head_() { printf '\n%s%s%s\n' "$BOLD" "$1" "$RESET"; }

# ── Host ─────────────────────────────────────────────────────────────────────
head_ "Host"

if [[ "$(uname -s)" != "Darwin" ]]; then
  bad "iOS builds require macOS (found $(uname -s))"
  hint "Apple does not ship the iOS SDK for any other platform."
  printf '\n%sCannot continue.%s\n' "$RED" "$RESET"
  exit 1
fi
ok "macOS $(sw_vers -productVersion) on $(uname -m)"

# ── Xcode ────────────────────────────────────────────────────────────────────
head_ "Xcode"

# `DEVELOPER_DIR` overrides `xcode-select` for every `xcrun`/`xcodebuild` call
# and, unlike `xcode-select -s`, needs no sudo. The scripts export it when the
# selected directory is wrong but Xcode is present, so an unprivileged checkout
# still builds. Honour the same precedence when reporting.
developer_dir="${DEVELOPER_DIR:-$(xcode-select -p 2>/dev/null || true)}"

if [[ -z "$developer_dir" ]]; then
  bad "No developer directory selected"
  hint "Install Xcode from the App Store, then: sudo xcode-select -s /Applications/Xcode.app"
elif [[ "$developer_dir" == *"CommandLineTools"* ]]; then
  if [[ -d "$XCODE_APP/Contents/Developer" ]]; then
    # Xcode is installed, just not selected. `DEVELOPER_DIR` covers our own
    # helper scripts, but NOT `tauri ios dev` — the Tauri CLI builds with a
    # sanitized environment and never forwards it, so `xcodebuild` still
    # resolves the Command Line Tools and fails. Only `xcode-select` fixes that,
    # so treat this as a hard failure rather than a warning.
    bad "$XCODE_APP is installed but Command Line Tools are still selected"
    hint "sudo xcode-select -s $XCODE_APP"
    hint "Required: 'tauri ios dev' ignores DEVELOPER_DIR, so this is the only fix."
    developer_dir="$XCODE_APP/Contents/Developer"
    export DEVELOPER_DIR="$developer_dir"
    ok "Using $developer_dir for the remaining checks"
  else
    bad "Command Line Tools are selected — these do not include the iOS SDK"
    hint "Install the full Xcode app: https://apps.apple.com/app/xcode/id497799835"
    hint "Then point the toolchain at it: sudo xcode-select -s $XCODE_APP"
  fi
else
  ok "Developer directory: $developer_dir"
fi

if [[ "$developer_dir" != *"CommandLineTools"* && -n "$developer_dir" ]]; then
  # The very first xcodebuild invocation after installing Xcode performs one-time
  # setup and can fail or time out; a second attempt succeeds. Retry once so a
  # cold machine does not block `ios:init` on a transient error.
  xcode_version="$(xcodebuild -version 2>/dev/null | head -1 || true)"
  if [[ -z "$xcode_version" ]]; then
    xcode_version="$(xcodebuild -version 2>/dev/null | head -1 || true)"
  fi
  if [[ -n "$xcode_version" ]]; then
    ok "$xcode_version"
  else
    bad "xcodebuild is not runnable"
    hint "Launch Xcode once to finish its first-run setup, then: sudo xcodebuild -runFirstLaunch"
  fi

  # The iOS *Simulator* SDK is the one the simulator build links against.
  if sim_sdk="$(xcrun --sdk iphonesimulator --show-sdk-version 2>/dev/null)"; then
    ok "iOS Simulator SDK $sim_sdk"
  else
    bad "iOS Simulator SDK not found"
    hint "Xcode → Settings → Components → install an iOS platform/simulator runtime."
  fi

  if ! xcodebuild -license check >/dev/null 2>&1; then
    warn "Xcode licence has not been accepted"
    hint "sudo xcodebuild -license accept"
  fi
fi

# ── Simulator runtimes and iPad devices ──────────────────────────────────────
head_ "Simulator"

if command -v xcrun >/dev/null 2>&1 && xcrun simctl help >/dev/null 2>&1; then
  ipad_types="$(xcrun simctl list devicetypes 2>/dev/null | grep -c 'iPad' || true)"
  if [[ "${ipad_types:-0}" -gt 0 ]]; then
    ok "$ipad_types iPad device type(s) available"
  else
    bad "No iPad device types found"
    hint "Xcode → Settings → Components → install an iOS Simulator runtime."
  fi

  # A device type is only usable once a matching runtime is *registered*.
  # Note the distinction from a downloaded disk image: a staged image whose
  # underlying asset is missing or unverified still appears under
  # `simctl runtime list` but registers no runtime, so no device can be created.
  # Report that case explicitly — the fix (purge and re-download) is different
  # from "nothing downloaded yet".
  runtimes="$(xcrun simctl list runtimes 2>/dev/null | grep -c 'iOS' || true)"
  if [[ "${runtimes:-0}" -gt 0 ]]; then
    ok "$runtimes iOS runtime(s) installed"
    ipad_devices="$(xcrun simctl list devices available 2>/dev/null | grep -c 'iPad' || true)"
    if [[ "${ipad_devices:-0}" -gt 0 ]]; then
      printf '%s' "$DIM"
      xcrun simctl list devices available 2>/dev/null | grep 'iPad' | sed 's/^ */      /' | head -8
      printf '%s' "$RESET"
    else
      warn "No iPad simulator devices created yet"
      hint "scripts/ios-simulator.sh creates one on demand."
    fi
  elif xcrun simctl runtime list 2>/dev/null | grep -q 'iOS'; then
    bad "An iOS runtime image is present but registers no usable runtime"
    hint "Purge and re-download it:"
    hint "xcrun simctl runtime delete all && xcodebuild -downloadPlatform iOS"
  else
    bad "No iOS simulator runtime installed"
    hint "xcodebuild -downloadPlatform iOS    (~8 GB)"
  fi
else
  bad "xcrun simctl unavailable (Xcode not installed or not selected)"
fi

# ── Rust toolchain ───────────────────────────────────────────────────────────
head_ "Rust"

if ! command -v rustup >/dev/null 2>&1; then
  bad "rustup not found"
  hint "curl --proto '=https' --tlsv1.2 https://sh.rustup.rs -sSf | sh"
else
  ok "$(rustc --version)"

  installed_targets="$(rustup target list --installed 2>/dev/null)"
  # aarch64-apple-ios     → physical iPad/iPhone
  # aarch64-apple-ios-sim → Simulator on Apple silicon
  # x86_64-apple-ios      → Simulator on Intel Macs
  required_targets=(aarch64-apple-ios aarch64-apple-ios-sim)
  [[ "$(uname -m)" == "x86_64" ]] && required_targets+=(x86_64-apple-ios)

  missing_targets=()
  for target in "${required_targets[@]}"; do
    if grep -qx "$target" <<<"$installed_targets"; then
      ok "target $target"
    else
      missing_targets+=("$target")
    fi
  done

  if [[ ${#missing_targets[@]} -gt 0 ]]; then
    if [[ $FIX -eq 1 ]]; then
      printf '  %s→%s installing: %s\n' "$YELLOW" "$RESET" "${missing_targets[*]}"
      if rustup target add "${missing_targets[@]}"; then
        for target in "${missing_targets[@]}"; do ok "target $target (installed)"; done
      else
        for target in "${missing_targets[@]}"; do bad "target $target (install failed)"; done
      fi
    else
      for target in "${missing_targets[@]}"; do
        bad "target $target is missing"
      done
      hint "rustup target add ${missing_targets[*]}   (or rerun with --fix)"
    fi
  fi
fi

# ── CocoaPods ────────────────────────────────────────────────────────────────
head_ "CocoaPods"

# Tauri's generated Xcode project resolves its dependencies through CocoaPods;
# `tauri ios init` shells out to `pod` and fails without it.
if command -v pod >/dev/null 2>&1; then
  ok "$(pod --version 2>/dev/null | head -1) ($(command -v pod))"
elif [[ $FIX -eq 1 ]] && command -v brew >/dev/null 2>&1; then
  printf '  %s→%s installing cocoapods via Homebrew\n' "$YELLOW" "$RESET"
  if brew install cocoapods; then ok "cocoapods installed"; else bad "cocoapods install failed"; fi
else
  bad "cocoapods not found"
  hint "brew install cocoapods    (or: sudo gem install cocoapods)"
fi

# ── Tauri CLI ────────────────────────────────────────────────────────────────
head_ "Tauri"

# pnpm keeps a workspace package's binaries under that package, not at the
# monorepo root, so @tauri-apps/cli lands in ui-desktop/node_modules/.bin.
if [[ -x ui-desktop/node_modules/.bin/tauri ]]; then
  ok "workspace CLI: $(ui-desktop/node_modules/.bin/tauri --version 2>/dev/null)"
elif command -v cargo-tauri >/dev/null 2>&1; then
  warn "workspace CLI missing; falling back to global $(cargo tauri --version 2>/dev/null)"
  hint "pnpm install"
else
  bad "Tauri CLI not available"
  hint "pnpm install    (installs @tauri-apps/cli into the workspace)"
fi

if [[ -d ui-desktop/src-tauri/gen/apple ]]; then
  ok "Xcode project generated at ui-desktop/src-tauri/gen/apple"
else
  warn "Xcode project not generated yet"
  hint "pnpm run ios:init"
fi

# ── Signing (physical devices only) ──────────────────────────────────────────
head_ "Signing"

# shellcheck source=lib/apple-team.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib/apple-team.sh"

teams=()
while IFS= read -r line; do
  if [[ -n "$line" ]]; then
    teams+=("$line")
  fi
done < <(detect_apple_teams)

override="${APPLE_DEVELOPMENT_TEAM:-${TAURI_APPLE_DEVELOPMENT_TEAM:-}}"

if [[ -n "$override" ]]; then
  ok "team $override (from the environment)"
elif [[ ${#teams[@]} -eq 1 ]]; then
  ok "team ${teams[0]} (from the keychain)"
elif [[ ${#teams[@]} -gt 1 ]]; then
  warn "several signing teams in the keychain: ${teams[*]}"
  hint "Pick one: APPLE_DEVELOPMENT_TEAM=<id> pnpm run ios:install"
else
  warn "no Apple signing certificate in the keychain"
  hint "The Simulator does not sign, so this only blocks builds for a real device."
  hint "A free Apple ID is enough: Xcode → Settings → Accounts → add your Apple ID,"
  hint "then Manage Certificates… → + → Apple Development."
fi

# ── Summary ──────────────────────────────────────────────────────────────────
printf '\n'
if [[ $failures -gt 0 ]]; then
  printf '%s%d check(s) failed%s' "$RED" "$failures" "$RESET"
  [[ $warnings -gt 0 ]] && printf ', %d warning(s)' "$warnings"
  printf '. Fix the items above, then rerun.\n'
  exit 1
fi

printf '%sReady to build for iOS%s' "$GREEN" "$RESET"
[[ $warnings -gt 0 ]] && printf ' (%d warning(s))' "$warnings"
printf '. Minimum deployment target: iOS %s.\n' "$MIN_IOS"
printf 'Next: %spnpm run ios:init%s, then %spnpm run ios:dev%s (simulator) or %spnpm run ios:install%s (real iPad)\n' \
  "$BOLD" "$RESET" "$BOLD" "$RESET" "$BOLD" "$RESET"
