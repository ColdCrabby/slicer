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

| Platform            | Start (background)                                                                     | Give the user            |
| ------------------- | ------------------------------------------------------------------------------------- | ------------------------ |
| Remote + web        | `cargo run -- serve` (backend, :5201) **and** `pnpm run ui:dev` (:4213)                | **http://localhost:4213** |
| Wasm browser slicer | `pnpm run hydrate:web-slicer` first, then `pnpm run ui:dev:web-slicer` (:4213)         | **http://localhost:4213** (no backend) |
| Tauri desktop       | `pnpm run desktop:dev`                                                                 | The app window opens — no URL |
| iOS / iPadOS        | `pnpm run ios:dev`                                                                     | The iPad simulator opens — no URL |
| CLI                 | Nothing to serve — give the exact command to run                                      | The command line          |

- **Reuse, don't stack.** If a dev server is already up on that port, say so and
  reuse it instead of starting a second.
- **Rebuild first when the change isn't live yet.** Wasm changes need
  `hydrate:web-slicer` (or `build:wasm`); a backend change needs the `serve`
  process (re)started. Make that the first thing you do, not a checklist bullet.
- **Report the real access detail.** On web, the UI dev server is on **:4213**
  and talks to the backend on **:5201** — the user opens :4213. On desktop/iOS
  the shell opens its own window, so there's no URL to give.
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
