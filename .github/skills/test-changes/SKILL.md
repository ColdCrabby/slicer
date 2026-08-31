---
name: test-changes
description: Start the parts needed to test a change, hand the user the access details (URL, etc.), then give a short, concise bullet-point checklist of what to test by hand — targeted at the platform they name — remote+web (the default), the wasm browser slicer, the Tauri desktop app, or iOS/iPadOS. Use when the user says "what should I test", "how do I test this", "give me a test plan", "let me test it", "I'll test it", "test on desktop / iPad / wasm", or asks what to check now that the change is done.
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
server — start it, do a quick liveness check, and move on.

**Every dev server runs on a random seed.** `pnpm run dev` rolls a three-digit
seed (200–999) and derives every port from it — UI on `4<seed>`, engine on
`5<seed>`, work directory `slicer-engine-dev-<seed>` — then checks they are
free. That is what keeps this session off the ports of the other checkout,
worktree or teammate already running on this host. **Never hardcode 4213/5201.**

| Platform            | Start (background)                                                              | Give the user                          |
| ------------------- | ------------------------------------------------------------------------------- | -------------------------------------- |
| Remote + web        | `pnpm run dev`                                                                  | **http://localhost:4\<seed\>/**        |
| Wasm browser slicer | `pnpm run hydrate:web-slicer` first, then `pnpm run dev:web-slicer`             | **http://localhost:4\<seed\>/** (no backend) |
| Tauri desktop       | `pnpm run dev:desktop`                                                          | The app window opens — no URL          |
| iOS / iPadOS        | `pnpm run ios:dev`                                                              | The iPad simulator opens — no URL      |
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
- **iOS is the one fixed-port flow.** `pnpm run ios:dev` uses the port pinned in
  `tauri.conf.json` because the generated Xcode project builds against it. If it
  is busy, that is the one case worth sorting out by hand.
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
| "iPad", "iOS", "iPadOS", "iPhone", "mobile", "simulator"  | iOS / iPadOS        |
| "CLI", "headless", "terminal"                             | CLI                 |

Plain "web" means remote + web — route to the wasm slicer only on an explicit
signal. If they name several platforms, write one block each and keep only what
is unique to that platform.

**Only ask for checks the chosen platform can actually prove.** These surfaces
differ in what runs where, so a passing check on one says nothing about another.
When the change matters somewhere the user isn't testing, note it in one line
rather than smuggling it into the list.

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
