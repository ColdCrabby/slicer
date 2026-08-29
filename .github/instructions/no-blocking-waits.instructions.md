---
description: "Use whenever a task could involve waiting — GitHub Actions or CI checks, PR status, releases, deploys, or any urge to sleep and poll. Never block on remote jobs; keep working, offload, or hand the wait back to the user."
name: "Never Wait: CI Checks and Blocking Sleeps"
---

# Never Wait on CI or Sleep to Poll

**Waiting is not work.** Time spent blocked on a remote job returns nothing to the user. Never spend it.

## Never

- `gh pr checks --watch`, `gh run watch`, or any `--watch` / `--wait` flag.
- `sleep` (or a long `initial_wait`) used to let a remote job progress.
- Polling loops — re-running a status command until the answer changes.
- Calling `read_agent` / `read_bash` repeatedly just to see whether something finished.

The runtime already notifies you when background agents and async shells complete, so polling is strictly slower than being told.

## Instead, in order of preference

1. **Keep working.** Move to any independent task — other files, other checks, docs, cleanup.
2. **Offload.** Put slow or long-running work in a background agent or async shell and continue elsewhere; you will be notified.
3. **Hand the wait to the user.** When there is genuinely nothing left to do, say what you are waiting on and end the turn. The user will reply when it finishes. That is the notification mechanism — do not simulate it with sleeps.

## Allowed

- **A single, immediate status check** (`gh pr checks`, `gh run list`) that returns right away and does not block.
- **Local commands that do real work** — builds, tests, installs. These are work, not waiting; give them the time they need.

The distinction: waiting on **your own command to compute something** is fine. Waiting on a **remote queue** is not.
