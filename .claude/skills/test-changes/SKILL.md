---
name: test-changes
description: Start the parts needed to test a change, hand the user the access details (URL, etc.), then give a short, concise bullet-point checklist of what to test by hand — targeted at the platform they name — remote+web (the default), the wasm browser slicer, the Tauri desktop app, or iOS/iPadOS (a real iPhone/iPad unless they ask for the simulator). Use when the user says "what should I test", "how do I test this", "give me a test plan", "let me test it", "I'll test it", "test on desktop / iPad / iPhone / wasm", or asks what to check now that the change is done.
---

# Get the User Testing

Do two things, in order: **stand up the thing to test**, then **write the
checklist**. The user does the testing; you start the servers and write the list.
Review the change you just made and turn it into the shortest set of hand-checks
that would actually catch a mistake in it, on the platform the user is sitting in
front of.

## Launch what's needed

Start the pieces the chosen platform needs as **background/detached** processes,
then hand the user the access details. Never block or poll waiting on a dev
server — start it, do a quick liveness check, and move on. The one exception is
a device install, which is a build that has to finish before there is anything
to test.

**Every dev server runs on a random seed.** `pnpm run dev` rolls a three-digit
seed (200–999) and derives every port from it — UI on `4<seed>`, engine on
`5<seed>`, work directory `slicer-engine-dev-<seed>` — then checks they are
free. That is what keeps this session off the ports of the other checkout,
worktree or teammate already running on this host. **Never hardcode 4213/5201.**

| Platform            | Start                                                                           | Give the user                          |
| ------------------- | ------------------------------------------------------------------------------- | -------------------------------------- |
| Remote + web        | `pnpm run dev` (background)                                                     | **http://localhost:4\<seed\>/**        |
| Wasm browser slicer | `pnpm run hydrate:web-slicer` first, then `pnpm run dev:web-slicer` (background) | **http://localhost:4\<seed\>/** (no backend) |
| Tauri desktop       | `pnpm run dev:desktop` (background)                                             | The app window opens — no URL          |
| iPhone / iPad       | `pnpm run ios:install` — a build; wait for it                                   | The app is on the device — no URL      |
| iOS Simulator       | `pnpm run ios:dev` (background)                                                 | The simulator opens — no URL           |
| CLI                 | Nothing to serve — give the exact command to run                                | The command line                       |

- **Read the seed back, don't guess it.** The launcher prints its banner first:

  ```
  Seed 742
    UI      http://localhost:4742/   <- open this
    Engine  http://127.0.0.1:5742/  (proxied at /api and /ws)
  ```

  Give the user **that** URL. `node scripts/dev.mjs --print` resolves a free
  seed and prints the ports as JSON without starting anything, and
  `pnpm run dev -- --seed 742` pins one when you need it stable across restarts.
- **One URL is all they need.** The dev server proxies `/api` and `/ws` to the
  engine, so the engine's port is an internal detail — never ask the user to
  open it.
- **Reuse your own, never someone else's.** If *this* session already has a
  stack up, reuse it. A server on some other port belongs to another instance —
  leave it alone and roll a fresh seed instead of killing it.
- **iOS means the real device unless they say "simulator".** Glass is the only
  place printers, touch, Pencil and on-device speed behave truthfully, and
  `ios:install` ships the **release** build — so it also catches production-only
  breakage a dev server hides. It is a build, not a server — ~4 minutes warm,
  longer on a cold cargo cache — so let it finish, or you are describing an app
  that is not installed yet.
- **Pre-flight the device.** `pnpm run ios:install --list` first; with two paired
  the script refuses to guess, so pass `--device '<name>'`. A first install also
  needs Developer Mode (Settings → Privacy & Security) and a one-time **Trust**
  (Settings → General → VPN & Device Management → Developer App) — the script
  catches the first, nothing warns about the second, so put it in the handover
  line. Then report the **expiry it prints**: a free signature lasts seven days,
  and `pnpm run ios:install --renew` resets the clock.
- **Iterating on UI? Live reload on hardware is `tauri ios dev --host`**, not
  `ios:dev`: `scripts/ios-dev.sh` always resolves a *simulator* name, so
  `ios:dev --host` still lands on the simulator. Live reload keeps the Mac in the
  loop (same network, dev server up); the release install does not.
- **iOS is the one fixed-port flow.** Both `pnpm run ios:dev` and
  `tauri ios dev --host` use the port pinned in `tauri.conf.json`, because the
  generated Xcode project builds against it. If it is busy, that is the one case
  worth sorting out by hand. `ios:install` needs no port at all.
- **Fresh workspace? Hydrate before anything else.** If `ui/src/generated/`
  (or `ui/src/generated/scene-wasm/`) is missing — a brand-new clone, or after
  `pnpm install` alone — every platform's UI build fails with `Cannot find
  module '../../generated/scene-wasm/scene_engine'`. Run `pnpm install &&
  pnpm run hydrate` (or `hydrate:web-slicer` for the wasm browser slicer) once,
  first, before starting any dev server.
- **Rebuild first when the change isn't live yet.** Wasm changes need
  `hydrate:web-slicer` (or `build:wasm`); a backend change needs the launcher
  restarted so `cargo run` picks it up. Make that the first thing you do, not a
  checklist bullet.
- Keep the startup note to a line or two, then go straight to the checklist.

## Write the checklist

## Rules

- **Bullets, not prose.** One line each, `action → expected result`. Cap at ~7.
  A paragraph gets skimmed; a short list gets run.
- **Only what changed.** Every bullet traces to the diff. Never pad to look
  thorough — cutting a weak bullet makes the list more likely to be run.
- **Include the likely failure**, not just the happy path, and lead with it.
- **Name the control the user sees**, not the code behind it.
- **Nothing user-visible?** Say so in one line instead of inventing checks.
- **Don't test it for them** unless they ask.

## Platform

Target the platform the user named. **If they didn't name one, assume
remote + web.**

| They say                                                  | Test on             |
| --------------------------------------------------------- | ------------------- |
| _nothing_, "web", "browser", "the UI", "server", "cloud"  | Remote + web        |
| "wasm", "web-slicer", "browser slicer", "no backend"      | Wasm browser slicer |
| "desktop", "tauri", "the app", "native"                   | Tauri desktop       |
| "iPad", "iOS", "iPadOS", "iPhone", "phone", "mobile"      | iPhone / iPad       |
| "simulator", "sim", "emulator", "I don't have it with me" | iOS Simulator       |
| "CLI", "headless", "terminal"                             | CLI                 |

Plain "web" means remote + web — route to the wasm slicer only on an explicit
signal. **Anything Apple-mobile means a real device**: route to the simulator
only when they ask for it by name, or when `--list` shows nothing connected — and
in that case say you fell back, so they can plug in instead. If they name several
platforms, write one block each and keep only what is unique to that platform.

**Only ask for checks the chosen platform can actually prove.** These surfaces
differ in what runs where, so a passing check on one says nothing about another.
When the change matters somewhere the user isn't testing, note it in one line
rather than smuggling it into the list.

Two Apple-specific splits are easy to get wrong:

- **The simulator cannot prove** LAN printers (it never shows the local-network
  prompt a device does), real touch or long-press, Apple Pencil, on-device
  slicing speed, or anything about signing and standalone operation. Send those
  checks to hardware or leave them out.
- **iPhone and iPad are different layouts, not different sizes.** A phone is
  under the 640px `handheld()` breakpoint and gets the tab bar, settings drawer
  and slice sheet; an iPad keeps the desktop layout. A layout change verified on
  one says nothing about the other — and if *only* the phone layout is in
  question, a narrowed browser window proves it in seconds instead of minutes.

If the change needs a rebuild before it is even running on that surface, make
that the first line — otherwise they test the old build.

## Format

```
**<Platform>**

- <Do this> → <expect this>.
- <Do this> → <expect this>.
```

Close with one line naming any automated coverage, so the user knows what they
_don't_ need to click.
