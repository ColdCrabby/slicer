#!/usr/bin/env bash
#
# ios-install.sh — put a standalone build on a physical iPhone or iPad.
#
# `pnpm run ios:dev` leaves the app pointed at the Angular dev server running on
# this Mac, so it goes blank the moment that process stops. This builds the
# *release* app instead: the whole UI is compiled into the binary and the
# slicing engine already runs on-device, so nothing is fetched at runtime. It
# then signs the result with a free Apple ID and installs it over the pairing
# you already have. Afterwards the device prints with no Mac anywhere in sight.
#
# The app is universal (iPhone and iPad), so this script does not care which one
# is plugged in. With more than one paired it refuses to guess — pass --device.
#
# No paid Apple Developer Program membership is involved. The trade is time: a
# free Apple ID signs for seven days, after which iOS refuses to launch the app
# until you run this again. `--renew` forces a fresh seven-day profile.
#
# Usage:
#   scripts/ios-install.sh                        # build, sign, install
#   scripts/ios-install.sh --list                 # show connected devices, then exit
#   scripts/ios-install.sh --device 'Max iPhone'  # pick a device by name
#   scripts/ios-install.sh --reinstall            # install the last build, skip building
#   scripts/ios-install.sh --renew                # discard the cached profile first
#   scripts/ios-install.sh --launch               # start the app once it is installed
#
# Overrides: IOS_DEVICE (device name), APPLE_DEVELOPMENT_TEAM (signing team ID).
set -euo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || echo .)"

readonly GEN_APPLE="ui-desktop/src-tauri/gen/apple"
readonly BUILD_DIR="$GEN_APPLE/build/arm64"
readonly TAURI_CONF="ui-desktop/src-tauri/tauri.conf.json"

if [[ -t 1 ]]; then
  BOLD=$'\033[1m'; DIM=$'\033[2m'; GREEN=$'\033[32m'; YELLOW=$'\033[33m'; RESET=$'\033[0m'
else
  BOLD=''; DIM=''; GREEN=''; YELLOW=''; RESET=''
fi

step() { printf '\n%s==>%s %s%s%s\n' "$GREEN" "$RESET" "$BOLD" "$1" "$RESET"; }
note() { printf '    %s%s%s\n' "$DIM" "$1" "$RESET"; }
die()  { printf 'error: %s\n' "$1" >&2; shift; for line in "$@"; do printf '       %s\n' "$line" >&2; done; exit 1; }

usage() {
  # Print the leading comment block (everything between the shebang and the
  # first line of code) as the usage text.
  awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
}

staging="$(mktemp -d -t ios-install)"

# Set once the build path is taken; see the DEVELOPMENT_TEAM note below.
pbxproj=""

cleanup() {
  if [[ -n "$pbxproj" && -f "$staging/project.pbxproj.orig" ]]; then
    cp "$staging/project.pbxproj.orig" "$pbxproj"
  fi
  rm -rf "$staging"
}
trap cleanup EXIT

do_build=1
do_renew=0
do_launch=0
list_only=0
requested_device="${IOS_DEVICE:-}"
team="${APPLE_DEVELOPMENT_TEAM:-${TAURI_APPLE_DEVELOPMENT_TEAM:-}}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help)   usage; exit 0 ;;
    # `pnpm run ios:install -- --renew` forwards the separator verbatim, so
    # accept it as the no-op it is rather than rejecting the documented form.
    --)          shift ;;
    --list)      list_only=1; shift ;;
    --reinstall) do_build=0; shift ;;
    --renew)     do_renew=1; shift ;;
    --launch)    do_launch=1; shift ;;
    --device)    requested_device="${2:?--device needs a name}"; shift 2 ;;
    --team)      team="${2:?--team needs a team ID}"; shift 2 ;;
    *)           die "unknown argument '$1'" "Run with --help for usage." ;;
  esac
done

# ── Toolchain ────────────────────────────────────────────────────────────────
#
# Same hard requirement as ios-dev.sh: the Tauri CLI shells out to `xcodebuild`
# with a sanitized environment, so `DEVELOPER_DIR` cannot rescue a checkout that
# still points at the Command Line Tools — and those ship no iOS SDK.
if [[ "$(xcode-select -p 2>/dev/null)" == *CommandLineTools* ]]; then
  if [[ -d /Applications/Xcode.app/Contents/Developer ]]; then
    die "Xcode is installed but the Command Line Tools are still selected." \
        "'tauri ios build' ignores DEVELOPER_DIR, so this must be fixed globally:" \
        "" \
        "  sudo xcode-select -s /Applications/Xcode.app"
  fi
  die "the full Xcode app is required (Command Line Tools have no iOS SDK)." \
      "Run scripts/ios-doctor.sh for the full diagnosis."
fi

[[ -d "$GEN_APPLE" ]] || die "the iOS Xcode project has not been generated yet." "Run: pnpm run ios:init"

xcodeproj="$(find "$GEN_APPLE" -maxdepth 1 -name '*.xcodeproj' -print -quit 2>/dev/null || true)"
[[ -n "$xcodeproj" ]] || die "no .xcodeproj found under $GEN_APPLE." "Run: pnpm run ios:init"

# The Angular build imports the WASM scene bindings and the generated types, and
# fails with an unrelated-looking module-resolution error when they are absent.
if [[ $do_build -eq 1 && ! -f ui/src/generated/scene-wasm/scene_engine.js ]]; then
  die "the generated WASM bindings are missing, so the UI cannot build." "Run: pnpm run hydrate"
fi

# ── Device ───────────────────────────────────────────────────────────────────
#
# `devicectl` replaced the old device tooling in Xcode 15; without it there is
# no supported way to install onto a device from a script.
xcrun devicectl --version >/dev/null 2>&1 || \
  die "'xcrun devicectl' is unavailable — Xcode 15 or newer is required to install onto a device." \
      "Run scripts/ios-doctor.sh for the full diagnosis."

# Prints "<identifier>\t<name>\t<model>\t<os>\t<pairing>\t<developer mode>" per
# connected iOS device, paired ones first — those are the ones we can install to.
list_ios_devices() {
  local json
  json="$(mktemp -t ios-devices)"
  xcrun devicectl list devices --json-output "$json" >/dev/null 2>&1 || true
  python3 - "$json" <<'PY'
import json, sys

try:
    with open(sys.argv[1]) as handle:
        data = json.load(handle)
except (OSError, ValueError):
    sys.exit(0)

rows = []
for device in data.get("result", {}).get("devices", []):
    hardware = device.get("hardwareProperties", {})
    props = device.get("deviceProperties", {})
    connection = device.get("connectionProperties", {})
    if hardware.get("platform") != "iOS":
        continue
    pairing = connection.get("pairingState", "unknown")
    rows.append((
        0 if pairing == "paired" else 1,
        props.get("name", "?"),
        [
            device.get("identifier", ""),
            props.get("name", "?"),
            hardware.get("marketingName") or hardware.get("deviceType", "?"),
            props.get("osVersionNumber", "?"),
            pairing,
            props.get("developerModeStatus", "unknown"),
        ],
    ))

rows.sort(key=lambda row: (row[0], row[1]))
for _, _, fields in rows:
    print("\t".join(fields))
PY
  rm -f "$json"
}

devices="$(list_ios_devices)"

if [[ $list_only -eq 1 ]]; then
  if [[ -z "$devices" ]]; then
    echo "No iOS devices are connected."
  else
    printf '%-28s %-24s %-8s %-10s %s\n' "DEVICE" "MODEL" "OS" "PAIRING" "DEVELOPER MODE"
    while IFS=$'\t' read -r _ name model os pairing devmode; do
      printf '%-28s %-24s %-8s %-10s %s\n' "$name" "$model" "$os" "$pairing" "$devmode"
    done <<<"$devices"
  fi
  exit 0
fi

if [[ -z "$devices" ]]; then
  die "no iOS device is connected." \
      "Plug the iPhone or iPad in (or keep it on the same Wi-Fi once paired)," \
      "unlock it, and tap Trust if it asks. Then: scripts/ios-install.sh --list"
fi

if [[ -n "$requested_device" ]]; then
  match="$(awk -F'\t' -v want="$requested_device" '$2 == want {print; exit}' <<<"$devices")"
  [[ -n "$match" ]] || die "no connected iOS device named '$requested_device'." \
      "Available: $(cut -f2 <<<"$devices" | paste -sd', ' -)"
else
  # Never guess between devices. Picking the first one alphabetically would send
  # a multi-minute build to the wrong phone/tablet and only say so in passing.
  paired_count="$(awk -F'\t' '$5 == "paired"' <<<"$devices" | wc -l | tr -d ' ')"
  if [[ "$paired_count" -gt 1 ]]; then
    printf 'error: %s connected devices are paired — name the one you want.\n' "$paired_count" >&2
    while IFS=$'\t' read -r _ name model _ pairing _; do
      [[ "$pairing" == "paired" ]] || continue
      printf "         scripts/ios-install.sh --device '%s'   # %s\n" "$name" "$model" >&2
    done <<<"$devices"
    exit 1
  fi
  match="$(head -1 <<<"$devices")"
fi

IFS=$'\t' read -r device_id device_name device_model device_os device_pairing device_devmode <<<"$match"

[[ "$device_pairing" == "paired" ]] || die "'$device_name' is not paired with this Mac ($device_pairing)." \
    "Connect it by cable, unlock it, and tap Trust This Computer."

# iOS 16+ will not run a locally-signed app until Developer Mode is on, and the
# toggle only appears after a signed build has been offered to the device once.
if [[ "$device_devmode" != "enabled" ]]; then
  die "Developer Mode is $device_devmode on '$device_name'." \
      "On the device: Settings → Privacy & Security → Developer Mode → on, then reboot." \
      "If that row is missing, connect the device to Xcode once to make it appear."
fi

# ── Signing team ─────────────────────────────────────────────────────────────
#
# The team ID is the Organizational Unit of the signing certificate, *not* the
# identifier printed in its common name. A free Apple ID gets a "personal team"
# that has no Membership page to read it off, so take it from the keychain.
# shellcheck source=lib/apple-team.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib/apple-team.sh"

if [[ -z "$team" ]]; then
  # `mapfile` would be tidier, but macOS still ships bash 3.2 and these scripts
  # have to run on a stock machine.
  detected=()
  while IFS= read -r line; do
    if [[ -n "$line" ]]; then
      detected+=("$line")
    fi
  done < <(detect_apple_teams)
  case ${#detected[@]} in
    0) die "no Apple signing certificate found in the keychain." \
           "Open Xcode → Settings → Accounts, add your Apple ID (free is fine)," \
           "then pick the team and press 'Manage Certificates…' → + → Apple Development." ;;
    1) team="${detected[0]}" ;;
    *) die "several signing teams found: ${detected[*]}" \
           "Choose one: APPLE_DEVELOPMENT_TEAM=<id> scripts/ios-install.sh" ;;
  esac
fi

bundle_id="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["identifier"])' "$TAURI_CONF")"

step "$bundle_id → $device_name"
note "$device_model · iOS $device_os · team $team"

# ── Renew ────────────────────────────────────────────────────────────────────
#
# Automatic signing reuses a cached profile while it is still valid, so a
# rebuild on day six inherits one day rather than starting a fresh week.
# Deleting the app's profiles makes Xcode mint a new seven-day one.
if [[ $do_renew -eq 1 ]]; then
  step "Discarding cached provisioning profiles for $bundle_id"
  removed=0
  for dir in "$HOME/Library/Developer/Xcode/UserData/Provisioning Profiles" \
             "$HOME/Library/MobileDevice/Provisioning Profiles"; do
    [[ -d "$dir" ]] || continue
    while IFS= read -r -d '' profile; do
      app_id="$(security cms -D -i "$profile" 2>/dev/null | plutil -extract Entitlements.application-identifier raw -o - - 2>/dev/null || true)"
      if [[ "$app_id" == *".$bundle_id" ]]; then
        rm -f "$profile"
        removed=$((removed + 1))
      fi
    done < <(find "$dir" -maxdepth 1 -name '*.mobileprovision' -print0 2>/dev/null)
  done
  note "$removed profile(s) removed"
fi

# ── Build ────────────────────────────────────────────────────────────────────
#
# `--export-method debugging` is Xcode's development export: the only one a free
# Apple ID can produce, and all a directly-installed app needs. The release
# build runs `beforeBuildCommand`, so the production UI is compiled into the
# binary and the app never looks for a dev server.
if [[ $do_build -eq 1 ]]; then
  [[ -f "$xcodeproj/project.xcworkspace/contents.xcworkspacedata" ]] || \
    die "$xcodeproj/project.xcworkspace is missing." \
        "'tauri ios build' passes it to xcodebuild -workspace. Regenerate it with:" \
        "  pnpm run ios:init"

  # The Tauri CLI writes DEVELOPMENT_TEAM into project.pbxproj on every build,
  # and that file is committed. Left alone it would put one contributor's team
  # ID in everybody else's checkout, so it is restored on exit — including when
  # a long build is interrupted. The team is supplied per-build through the
  # environment and does not belong in the repository.
  pbxproj="$xcodeproj/project.pbxproj"

  # An interrupt before this existed can have left the line behind, and
  # snapshotting *that* would preserve it forever. Drop it first, but only when
  # git confirms nothing else in the file differs from HEAD.
  if git rev-parse --git-dir >/dev/null 2>&1 && ! git diff --quiet -- "$pbxproj" 2>/dev/null; then
    if [[ -z "$(git diff -U0 -- "$pbxproj" | sed -n '/^[+-][^+-]/p' | grep -v DEVELOPMENT_TEAM)" ]]; then
      git checkout -- "$pbxproj"
      note "cleared a DEVELOPMENT_TEAM line left over from an interrupted run"
    else
      note "$pbxproj has local edits; they will be preserved"
    fi
  fi

  cp "$pbxproj" "$staging/project.pbxproj.orig"

  step "Building the release app (this takes a while on a cold cache)"
  build_status=0
  APPLE_DEVELOPMENT_TEAM="$team" \
    pnpm --filter slicer-ui-desktop exec tauri ios build \
      --target aarch64 --export-method debugging || build_status=$?

  if [[ $build_status -ne 0 ]]; then
    printf '\n' >&2
    die "the iOS build failed." \
        "Common causes, in the order they bite:" \
        "  • Xcode has no Apple ID — add one under Xcode → Settings → Accounts." \
        "  • '$bundle_id' is registered to somebody else's team. Bundle IDs are" \
        "    globally unique, so change \"identifier\" in $TAURI_CONF" \
        "    to something of your own and re-run: pnpm run ios:init" \
        "  • The free tier allows 10 app IDs per 7 days and 3 apps per device."
  fi
fi

ipa="$(ls -t "$BUILD_DIR"/*.ipa 2>/dev/null | head -1 || true)"
[[ -n "$ipa" ]] || die "no .ipa found in $BUILD_DIR." \
    "Run without --reinstall to build one."

# ── Install ──────────────────────────────────────────────────────────────────
unzip -q "$ipa" -d "$staging"
app="$(find "$staging/Payload" -maxdepth 1 -name '*.app' -print -quit 2>/dev/null || true)"
[[ -n "$app" ]] || die "'$ipa' contains no .app bundle."

step "Installing $(basename "$app") on $device_name"
xcrun devicectl device install app --device "$device_id" "$app"

if [[ $do_launch -eq 1 ]]; then
  step "Launching"
  # A never-trusted developer certificate makes this fail with a bare "app is
  # damaged"-style error, which is expected on a first install — say so instead
  # of failing the whole run.
  xcrun devicectl device process launch --device "$device_id" "$bundle_id" || {
    note "Could not launch — trust the developer certificate first (see below)."
  }
fi

# ── Expiry ───────────────────────────────────────────────────────────────────
#
# The seven-day clock is the whole reason this script exists, so report the
# actual date rather than leaving the user to find out when the app stops.
expires=""
profile="$app/embedded.mobileprovision"
if [[ -f "$profile" ]]; then
  raw="$(security cms -D -i "$profile" 2>/dev/null | plutil -extract ExpirationDate raw -o - - 2>/dev/null || true)"
  # "2026-09-07T13:24:19Z" — drop the zone marker and any fractional seconds so
  # BSD `date` can parse it.
  stamp="${raw%Z}"
  stamp="${stamp%%.*}"
  if [[ -n "$stamp" ]] && epoch="$(date -j -u -f '%Y-%m-%dT%H:%M:%S' "$stamp" '+%s' 2>/dev/null)"; then
    remaining=$(( epoch - $(date +%s) ))
    if [[ $remaining -le 0 ]]; then
      expires="$(date -r "$epoch" '+%a %-d %b %Y, %H:%M') — already expired"
    else
      # Round *up*: a profile minted moments ago has 6 days 23 hours left, and
      # truncating that to "6" makes a fresh signature look like a stale one.
      days=$(( (remaining + 86399) / 86400 ))
      expires="$(date -r "$epoch" '+%a %-d %b %Y, %H:%M') ($days day(s) left)"
    fi
  fi
fi

printf '\n%sInstalled.%s ' "$GREEN$BOLD" "$RESET"
if [[ -n "$expires" ]]; then
  printf 'Signed until %s%s%s\n' "$BOLD" "$expires" "$RESET"
else
  printf 'A free Apple ID signs for 7 days.\n'
fi

cat <<EOF

${BOLD}On the iPad, once:${RESET}
  Settings → General → VPN & Device Management → Developer App → Trust

${BOLD}When it expires:${RESET}
  scripts/ios-install.sh --renew        ${DIM}# rebuild, re-sign, reinstall${RESET}
  ${DIM}Your models, profiles and settings stay put — the app is replaced, not removed.${RESET}
EOF

if [[ "$(cut -f1 -d$'\t' <<<"$devices" | wc -l | tr -d ' ')" -gt 1 ]]; then
  printf '\n%sOther connected devices:%s scripts/ios-install.sh --list\n' "$YELLOW" "$RESET"
fi
