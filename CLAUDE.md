This file is an index, not a reference. Don't restate what these already say — open them.

## Read first

@AGENTS.md

It is a **map**: one line per subsystem, pointing at the README that owns the
depth. Follow the pointer rather than assuming the map is the whole story.

Also on hand: [ARCHITECTURE.md](ARCHITECTURE.md) (high-level design map),
[DEVELOPMENT.md](DEVELOPMENT.md) (dev workflow, common tasks),
[CONTRIBUTING.md](CONTRIBUTING.md) (contribution process), and one `README.md`
per `src/*/` module, plus [ui/README.md](ui/README.md) and
[ui-desktop/README.md](ui-desktop/README.md).

## Always on

Two rules apply to every task, so they are stated here rather than behind a skill:

- **No issue or PR numbers in prose.** Not in commit messages, PR titles,
  CHANGELOG entries, READMEs, comments, doc comments or UI copy. Describe the
  change, not its paperwork. Detail:
  [`no-issue-numbers`](.github/instructions/no-issue-numbers.instructions.md).
- **Never block on a remote job.** No `--watch` / `--wait` flags, no `sleep` to
  let CI progress, no polling loops. The runtime notifies you when background
  work finishes. Detail:
  [`no-blocking-waits`](.github/instructions/no-blocking-waits.instructions.md).

## Task-specific instructions

`.github/instructions/*.instructions.md` hold the rules for particular kinds of
work. They are **one copy, shared with GitHub Copilot** — Copilot applies them
automatically via `applyTo`; you reach them through the skills below, or by
opening them directly. Read the matching one instead of improvising the same rule.

| Instruction | Applies to |
| --- | --- |
| [`ui-design-language`](.github/instructions/ui-design-language.instructions.md) | Styling, theming, tokens, visual polish in `ui/` |
| [`progressive-disclosure`](.github/instructions/progressive-disclosure.instructions.md) | Exposing a setting or capability — tiers, `Automatic`, honest help text |
| [`angular-component-structure`](.github/instructions/angular-component-structure.instructions.md) | Creating, splitting or reviewing Angular components |
| [`ui-style-no-build`](.github/instructions/ui-style-no-build.instructions.md) | Whether to run a build to verify UI work |
| [`slicing-visual-verification`](.github/instructions/slicing-visual-verification.instructions.md) | Any change that alters sliced geometry |
| [`no-issue-numbers`](.github/instructions/no-issue-numbers.instructions.md) | Any prose a human reads |
| [`no-blocking-waits`](.github/instructions/no-blocking-waits.instructions.md) | Anything that could involve waiting on CI |

## Specialized agents and skills

Claude auto-discovers these — no need to invoke them manually:

- `.claude/agents/*.md` — specialized subagents (Senior Slicer Engineer,
  Documentation Sync, Senior Three.js Engineer), mirrored for GitHub Copilot
  at `.github/agents/*.agent.md`.
- `.claude/skills/*/SKILL.md`:
  - **`ui-ux`** — routes to the four UI instruction files and carries the rules
    most often broken. Fires on any `ui/src` work.
  - **`slicing-verification`** — before/after picture workflow and the
    G-code measurement tools, for geometry changes.
  - **`release`** — curate the changelog, tag, push.
  - **`test-changes`** — stand up what's needed and write the hand-test checklist.
