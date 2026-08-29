---
name: test-changes
description: Hand the user a short, concise bullet-point checklist of what to test by hand after a change, targeted at the platform they name — remote+web (the default), the wasm browser slicer, the Tauri desktop app, or iOS/iPadOS. Use when the user says "what should I test", "how do I test this", "give me a test plan", "let me test it", "I'll test it", "test on desktop / iPad / wasm", or asks what to check now that the change is done.
---

# Suggest What to Test

**The user does the testing. You write the checklist.** Turn the change you just
made into the shortest list of hand-checks that would actually catch a mistake in
it, on the surface the user is sitting in front of.

Two things make or break this skill:

- **Bullets, not prose.** A paragraph gets skimmed and ignored. A five-bullet
  list gets run.
- **The right platform.** Most of this repo's bugs live on exactly one surface —
  a CORS failure the desktop build can never reproduce, a UIKit popover crash no
  browser can, a stale wasm bundle that quietly serves yesterday's engine.
  Testing the wrong surface proves nothing.

## Guardrails

- **Never invent a check.** Every bullet traces to a real hunk in the diff. If
  you didn't touch it, don't ask them to click it.
- **Never pad to look thorough.** One real change → one bullet. Cap at ~7.
  Cutting a weak bullet makes the list _more_ likely to be run, not less.
- **Never claim it works.** You are asking the user to find out. Write checks,
  not reassurance.
- **Don't run the app for them** unless they ask. Give the command; let them
  drive. (Do offer if they seem to want it.)
- **Say so when there's nothing to test.** If the change is internal and covered
  by `cargo test`, one honest line beats a fabricated checklist.

---

## 1. Pick the platform

**If the user didn't say, assume remote + web.**

| They say                                                              | Platform          | Start it with                                                             | Runtime mode      |
| --------------------------------------------------------------------- | ----------------- | ------------------------------------------------------------------------- | ----------------- |
| _nothing_, "web", "browser", "the UI", "server", "cloud", "remote"     | **Remote + web**  | `cargo run -- serve` **and** `pnpm run ui:dev` → http://localhost:4200     | `cloud`           |
| "wasm", "web-slicer", "browser slicer", "no backend", "the demo site"  | **Wasm slicer**   | `pnpm run hydrate:web-slicer` then `pnpm run ui:dev:web-slicer`            | `web`             |
| "desktop", "tauri", "the app", "native", "mac/windows/linux app"       | **Tauri desktop** | `pnpm run desktop:dev`                                                    | `native`          |
| "iPad", "iOS", "iPadOS", "iPhone", "simulator", "mobile", "tablet"     | **iOS / iPadOS**  | `pnpm run ios:dev` (macOS + full Xcode; `pnpm run ios:doctor` if it fails) | `native` + mobile |
| "CLI", "headless", "terminal", "command line"                          | **CLI**           | `cargo run -- slice -i 3DBenchy.stl -o /tmp/out.gcode`                     | —                 |

**"web" alone means remote + web, not the wasm slicer.** Both render in a
browser and the wasm build's runtime mode is literally called `web`, so this is
easy to get backwards. Only route to the wasm slicer on an explicit signal
("wasm", "web-slicer", "no backend"). Ambiguous "native" → desktop; iPad users
say iPad.

If they name several platforms, write one block each — shared checks go once
under **All platforms**, and each block carries only what is unique to it.

---

## 2. Read what actually changed

```bash
git diff --stat $(git merge-base HEAD origin/main)..HEAD    # or: git diff --stat
git diff $(git merge-base HEAD origin/main)..HEAD -- <the interesting paths>
```

Map the changed paths onto things a human can see:

| Changed                                                | What the user should exercise                                                        |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `src/core/`, `src/walls/`, `src/infill/`, `src/gcode/` | Slice a real model and **look at the beads** — see §6                                |
| `src/adhesion/`                                        | Slice with skirt / brim / raft on; check layer 1 and that the object didn't shift     |
| `src/scene/`                                           | Add, move, rotate, scale, center, drop-to-floor, align-face in the viewer; undo/redo  |
| `src/server/`                                          | Connect banner, run a slice, history list, re-slice the same scene (cache hit)        |
| `src/profiles/`                                        | Create / edit / delete a printer, filament, process, label; reload the page           |
| `src/printer/`                                         | Home-page status dot, "detect", send G-code to a real or fake host                    |
| `src/db/`                                              | History entries; re-slicing an identical scene should return instantly                |
| `src/cli/`                                             | The command plus `--help`; `info`, `changelog`                                        |
| `src/version.rs`, `CHANGELOG.md`                       | `--version`, `info`, and the "What's New" dialog on upgrade                            |
| `ui/src/app/**`                                        | The exact screen or control touched — name it, don't say "the UI"                     |
| `ui/src/styles/**`, theme tokens                       | Light **and** dark, accent colour, a narrow window                                    |
| `ui-desktop/src-tauri/**`                              | **Desktop and iOS both** — they share one shell                                       |

---

## 3. Make sure they're testing the new code

The engine runs in a different process on every surface, so "I rebuilt" means
something different each time. Lead the checklist with a rebuild line only when
the diff makes it necessary.

| Changed                         | Testing on     | They must first                                                      |
| ------------------------------- | -------------- | -------------------------------------------------------------------- |
| anything in `src/scene/`        | any UI surface | `pnpm run hydrate` — the viewer runs scene code as **wasm**          |
| a type that feeds a JSON schema | any UI surface | `pnpm run gen` (never hand-edit `ui/src/generated/`)                 |
| slicing / engine Rust           | remote + web   | restart `cargo run -- serve` — the browser didn't change             |
| slicing / engine Rust           | wasm slicer    | `pnpm run hydrate:web-slicer` — the engine ships _in the bundle_     |
| slicing / engine Rust           | desktop / iOS  | Tauri rebuilds on save; if it didn't, restart `pnpm run desktop:dev` |
| UI TypeScript / SCSS only       | any            | nothing — HMR has it                                                 |

A stale bundle is the most common way a user "tests" a change that isn't
running. When in doubt, say which rebuild to run.

---

## 4. Write the bullets

Format, exactly:

```
**<Platform>** — `<command to start it>`

- <Do this> → <expect this>.
- <Do this> → <expect this>.

⚠️ <one line, only if a real trap applies>
```

Rules for each bullet:

- **One line. Action → expectation.** The user must be able to tell pass from
  fail without asking you.
- **Name the real control**, not the code ("the printer card's status dot", not
  `PrinterConnection.status`).
- **Include the failure case**, not just the happy path — that's where the bug
  is.
- **Lead with the bullet most likely to fail.**
- Add `⚠️` only for a genuine trap (a check this platform _cannot_ prove, a known
  sharp edge). Never as filler.

Close with automated coverage as a **footnote, not bullets** — one line:
"`cargo test walls::arachne` already covers X". It tells the user what they
_don't_ need to click.

### Worked example

Change: the printer status dot now distinguishes CORS-blocked from offline.

```
**Remote + web** — `cargo run -- serve` + `pnpm run ui:dev` → http://localhost:4200

- Add a printer pointing at a reachable Moonraker host → dot goes green.
- Point one at a dead IP → dot goes red, not amber.
- Stop the engine mid-check → dot settles amber, no console errors.

⚠️ The CORS branch doesn't exist here — probes go over the WebSocket. A green dot
on this surface says nothing about the browser-fetch path; test wasm for that.

`cargo test printer::` covers the status mapping itself.
```

Nothing user-visible? Say that instead:

> Internal to `prune_redundant_gap_fill` and covered by `cargo test core::surfaces`.
> Nothing to click — the meaningful check is the before/after bead render.

---

## 5. Platform traps worth a bullet

Include these only when the diff actually touches them.

**Remote + web** — the default, and the widest surface.

- WebSocket drop and reconnect; the connect banner must recover.
- Slicing the **same scene twice** should hit the G-code cache and return
  instantly — while a version bump or reordered objects must _miss_ it.
- Profiles round-trip through the engine: edit, hard-reload, still there.
- A **second browser tab** must refresh profiles on `ProfilesChanged`.
- The prod flow needs the UI built first, or `serve` fails on a missing
  `ui/dist`.

**Wasm browser slicer** — no backend, no native transport.

- The whole slice runs in the tab: try a **big model** and watch time and memory.
- Printer probes fall back to browser `fetch`, so a reachable printer can still
  report `cors` — expected here and **only** here.
- Profiles live in `localStorage` only: clearing site data really does lose them.
- Confirm it is actually the web-slicer build, not the cloud dev server on the
  same port.

**Tauri desktop** — native shell, `cloud` environment.

- The desktop build ships the `cloud` env and becomes native only by **detecting
  Tauri at runtime** — verify it took the native path (Tauri commands, no CORS).
- Window chrome: minimize / maximize / close / drag, and right-click menus.
- Profiles land in `profiles.toml` in the config dir, not `localStorage`.
- Re-check anything shared with iOS — one shell, two platforms.

**iOS / iPadOS** — everything is a popover and half the API doesn't exist.

- **Long-press** is the right-click: it must open a native action sheet, and the
  finger-lift must _not_ activate whatever is underneath.
- Confirm/alert dialogs must be native, not the HTML fallback.
- Export G-code goes through the **share sheet** — a 0-byte file means it fell
  back to `save()`.
- Import must offer `obj` and `3mf`, not just `stl` (greyed out = missing UTIs).
- A popover without an anchor **crashes the app** — open every sheet on iPad.
- LAN printers need the local-network prompt; the first probe should ask.
- There is no CLI, server, or history/DB on iOS — don't ask for them.
- If it renders desktop chrome, something asked `isTauriHost()` where it needed
  `isTauriDesktop()`.

**CLI** — the fastest way to test the pipeline itself.

- Run the command plus `--help`; check exit codes and the error text on bad
  input.
- `--version` / `info` should report the expected version.

---

## 6. Slicing-geometry changes need a picture, not a click

If the diff touches `src/core/`, `src/walls/`, `src/infill/`, `src/adhesion/` or
`src/gcode/`, no UI checklist can verify it. Follow
[`slicing-visual-verification.instructions.md`](../../instructions/slicing-visual-verification.instructions.md):
slice before and after with identical settings, render real beads with
`tools/gcode-analysis/beaddiff.py`, and say which layer to look at.

Real models sit at the repo root — `3DBenchy.stl`, `Voron_Design_Cube_v7.stl`,
`Filament_Card_Caddy_25.stl`. Name **which** model and **which** layer; "slice
something" is not a test.

Suggest the numeric tools when the change claims a specific win: `voids.py`
(unfilled gaps), `overlap.py` (double extrusion), `widthdist.py` (bead widths).

---

## See also

- [SETUP.md](../../../SETUP.md) — every surface's setup and run commands
- [ui-desktop/README.md](../../../ui-desktop/README.md) — which surfaces are native, and why
- [AGENTS.md](../../../AGENTS.md) — runtime-mode contracts, printer transport, profile store
- [tools/gcode-analysis/](../../../tools/gcode-analysis/README.md) — G-code quality diagnostics
