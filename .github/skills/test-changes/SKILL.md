---
name: test-changes
description: Hand the user a short, concise bullet-point checklist of what to test by hand after a change, targeted at the platform they name — remote+web (the default), the wasm browser slicer, the Tauri desktop app, or iOS/iPadOS. Use when the user says "what should I test", "how do I test this", "give me a test plan", "let me test it", "I'll test it", "test on desktop / iPad / wasm", or asks what to check now that the change is done.
---

# Suggest What to Test

The user does the testing; you write the list. Review the change you just made
and turn it into the shortest set of hand-checks that would actually catch a
mistake in it, on the platform the user is sitting in front of.

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
