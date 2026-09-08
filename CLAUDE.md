This file is an index, not a reference. Don't restate what these already say — open them.

## Read first

@AGENTS.md

Also on hand: [ARCHITECTURE.md](ARCHITECTURE.md) (high-level design map),
[DEVELOPMENT.md](DEVELOPMENT.md) (dev workflow, common tasks),
[CONTRIBUTING.md](CONTRIBUTING.md) (contribution process), and one `README.md`
per `src/*/` module.

## Task-specific instructions

`.github/instructions/*.instructions.md` — shared with GitHub Copilot, not
Claude-specific. Each file's frontmatter says when it applies; read the
matching one instead of improvising the same rule:

- [`no-issue-numbers.instructions.md`](.github/instructions/no-issue-numbers.instructions.md)
- [`no-blocking-waits.instructions.md`](.github/instructions/no-blocking-waits.instructions.md)
- [`slicing-visual-verification.instructions.md`](.github/instructions/slicing-visual-verification.instructions.md)
- [`ui-design-language.instructions.md`](.github/instructions/ui-design-language.instructions.md)
- [`angular-component-structure.instructions.md`](.github/instructions/angular-component-structure.instructions.md)
- [`ui-style-no-build.instructions.md`](.github/instructions/ui-style-no-build.instructions.md)

## Specialized agents and skills

Claude auto-discovers these — no need to invoke them manually:

- `.claude/agents/*.md` — specialized subagents (Senior Slicer Engineer,
  Documentation Sync, Senior Three.js Engineer), mirrored for GitHub Copilot
  at `.github/agents/*.agent.md`.
- `.claude/skills/*/SKILL.md` — `release` and `test-changes`.
