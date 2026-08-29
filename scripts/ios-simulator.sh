#!/usr/bin/env bash
#
# ios-simulator.sh — resolve, boot and show an iPad simulator.
#
# `tauri ios dev` with no argument tries a physical device first and otherwise
# drops into an interactive picker that lists every simulator — mostly iPhones.
# Since iPad is the target form factor, this script picks a sensible iPad for
# you and is the device resolver behind `pnpm run ios:dev`.
#
# Usage:
#   scripts/ios-simulator.sh                    # boot the default iPad, open Simulator.app
#   scripts/ios-simulator.sh --list             # list available iPad simulators
#   scripts/ios-simulator.sh --name             # print the resolved device name only
#   scripts/ios-simulator.sh 'iPad Pro 13-inch (M4)'
#
# Override the default for any of these with IOS_DEVICE='<simulator name>'.
set -euo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || echo .)"

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  # Print the leading comment block (everything between the shebang and the
  # first line of code) as the usage text.
  awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
  exit 0
fi

# Point at the full Xcode app when the selected developer directory is the
# Command Line Tools, which ship no iOS SDK and no Simulator. `DEVELOPER_DIR`
# does this per-process and — unlike `sudo xcode-select -s` — needs no
# privileges, which is enough for the simctl calls below.
#
# It is NOT enough for `tauri ios dev`: the Tauri CLI builds with a sanitized
# environment and never forwards `DEVELOPER_DIR`, so its `xcodebuild` still
# resolves the Command Line Tools. That case needs `sudo xcode-select -s`, and
# ios-doctor.sh reports it as a hard failure.
if [[ "$(xcode-select -p 2>/dev/null)" == *CommandLineTools* &&
      -d /Applications/Xcode.app/Contents/Developer ]]; then
  export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
fi

if ! xcrun simctl help >/dev/null 2>&1; then
  echo "error: 'xcrun simctl' is unavailable — Xcode is not installed or not selected." >&2
  echo "       Run scripts/ios-doctor.sh for the full diagnosis." >&2
  exit 1
fi

# Prints "<udid>\t<name>\t<runtime>" for every available iPad, best first.
#
# "Best" = newest iOS runtime, then by family preference (Pro > Air > mini >
# plain iPad), so a fresh checkout lands on a large-screen device that matches
# how the slicer UI is meant to be used.
list_ipads() {
  xcrun simctl list devices available --json 2>/dev/null | python3 -c '
import json, re, sys

FAMILY_RANK = {"ipad pro": 0, "ipad air": 1, "ipad (": 2, "ipad mini": 3}

def family(name):
    lowered = name.lower()
    for key, rank in FAMILY_RANK.items():
        if lowered.startswith(key):
            return rank
    return 9

def screen_inches(name):
    # "iPad Pro 13-inch (M4)" -> 13.0; unknown sizes sort last within a family.
    match = re.search(r"([\d.]+)-inch", name)
    return float(match.group(1)) if match else 0.0

def runtime_version(identifier):
    # com.apple.CoreSimulator.SimRuntime.iOS-18-2 -> (18, 2)
    match = re.search(r"iOS-([\d-]+)$", identifier)
    if not match:
        return (0,)
    return tuple(int(part) for part in match.group(1).split("-"))

rows = []
for runtime, devices in json.load(sys.stdin).get("devices", {}).items():
    if "SimRuntime.iOS-" not in runtime:
        continue
    version = runtime_version(runtime)
    label = "iOS " + ".".join(str(part) for part in version)
    for device in devices:
        if not device.get("isAvailable", False):
            continue
        name = device.get("name", "")
        if "ipad" not in name.lower():
            continue
        rows.append((version, family(name), screen_inches(name), name, device["udid"], label))

# Newest runtime first, then family preference, then the largest screen (the
# slicer UI is laid out for a big canvas), then name for stability.
rows.sort(key=lambda row: (tuple(-part for part in row[0]), row[1], -row[2], row[3]))
for _, _, _, name, udid, label in rows:
    print(f"{udid}\t{name}\t{label}")
'
}

ipads="$(list_ipads)"

# A freshly-installed runtime ships device *types* but no created devices, so
# the list is legitimately empty until something creates one. Do that here
# rather than making the user run `simctl create` by hand: pick the newest iOS
# runtime and the best iPad type available under the same ranking used above.
if [[ -z "$ipads" ]]; then
  created="$(xcrun simctl list --json 2>/dev/null | python3 -c '
import json, re, sys

FAMILY_RANK = {"ipad pro": 0, "ipad air": 1, "ipad (": 2, "ipad mini": 3}

def family(name):
    lowered = name.lower()
    for key, rank in FAMILY_RANK.items():
        if lowered.startswith(key):
            return rank
    return 9

def screen_inches(name):
    match = re.search(r"([\d.]+)-inch", name)
    return float(match.group(1)) if match else 0.0

def version_of(identifier):
    match = re.search(r"iOS-([\d-]+)$", identifier)
    if not match:
        return (0,)
    return tuple(int(part) for part in match.group(1).split("-"))

data = json.load(sys.stdin)

runtimes = [
    r for r in data.get("runtimes", [])
    if r.get("isAvailable") and "SimRuntime.iOS-" in r.get("identifier", "")
]
if not runtimes:
    sys.exit(0)
runtime = max(runtimes, key=lambda r: version_of(r["identifier"]))

# Only device types the chosen runtime actually supports can be created.
supported = {
    d.get("identifier")
    for d in data.get("devicetypes", [])
    if "iPad" in d.get("name", "")
}
usable = [
    d for d in data.get("devicetypes", [])
    if d.get("identifier") in supported
]
if not usable:
    sys.exit(0)
best = min(usable, key=lambda d: (family(d["name"]), -screen_inches(d["name"]), d["name"]))
print(f'{best["identifier"]}\t{runtime["identifier"]}\t{best["name"]}')
')"

  if [[ -n "$created" ]]; then
    IFS=$'\t' read -r type_id runtime_id type_name <<<"$created"
    echo "No iPad simulator existed yet — creating \"$type_name\"…"
    xcrun simctl create "$type_name" "$type_id" "$runtime_id" >/dev/null 2>&1 || true
    ipads="$(list_ipads)"
  fi
fi

if [[ -z "$ipads" ]]; then
  echo "error: no available iPad simulators found." >&2
  echo "       Install an iOS runtime: xcodebuild -downloadPlatform iOS" >&2
  echo "       Then run scripts/ios-doctor.sh to confirm it registered." >&2
  exit 1
fi

case "${1:-}" in
  --list)
    printf '%-40s %s\n' "DEVICE" "RUNTIME"
    while IFS=$'\t' read -r _ name runtime; do
      printf '%-40s %s\n' "$name" "$runtime"
    done <<<"$ipads"
    exit 0
    ;;
  # `--name` is the scripting hook: print the resolved name and change nothing,
  # so callers can hand it straight to `tauri ios dev "<name>"`.
  --name)
    name_only=1
    requested="${IOS_DEVICE:-}"
    ;;
  *)
    name_only=0
    requested="${1:-${IOS_DEVICE:-}}"
    ;;
esac

if [[ -n "$requested" ]]; then
  match="$(awk -F'\t' -v want="$requested" '$2 == want {print; exit}' <<<"$ipads")"
  if [[ -z "$match" ]]; then
    echo "error: no available iPad simulator named '$requested'." >&2
    echo "       Available:" >&2
    awk -F'\t' '{print "         " $2 "  (" $3 ")"}' <<<"$ipads" >&2
    exit 1
  fi
else
  match="$(head -1 <<<"$ipads")"
fi

IFS=$'\t' read -r udid name runtime <<<"$match"

if [[ "$name_only" -eq 1 ]]; then
  printf '%s\n' "$name"
  exit 0
fi

echo "Booting $name ($runtime)…"
# `boot` exits non-zero when the device is already booted; that is a success here.
xcrun simctl boot "$udid" 2>/dev/null || true
open -a Simulator
xcrun simctl bootstatus "$udid" -b >/dev/null 2>&1 || true
echo "Ready. Run: pnpm run ios:dev"
