# Slicer Engine — AI Agent Instructions

This file is an index, not a reference. Don't restate what these already say —
open them.

## Read first

- **[AGENTS.md](../AGENTS.md)** — architecture, pipeline, module contracts,
  build/test commands, constraints. The primary reference for all agents.
- **[ARCHITECTURE.md](../ARCHITECTURE.md)** — high-level design map.
- **[DEVELOPMENT.md](../DEVELOPMENT.md)** — dev workflow, common tasks.
- **[CONTRIBUTING.md](../CONTRIBUTING.md)** — contribution process.
- **Module `README.md`s** — one per `src/*/` directory.

## Task-specific instructions

`.github/instructions/*.instructions.md` — shared with Copilot, not
Claude-specific. Each file's frontmatter says when it applies; read the
matching one instead of improvising the same rule:

- [`no-issue-numbers.instructions.md`](../.github/instructions/no-issue-numbers.instructions.md)
- [`no-blocking-waits.instructions.md`](../.github/instructions/no-blocking-waits.instructions.md)
- [`slicing-visual-verification.instructions.md`](../.github/instructions/slicing-visual-verification.instructions.md)
- [`ui-design-language.instructions.md`](../.github/instructions/ui-design-language.instructions.md)
- [`angular-component-structure.instructions.md`](../.github/instructions/angular-component-structure.instructions.md)
- [`ui-style-no-build.instructions.md`](../.github/instructions/ui-style-no-build.instructions.md)

## Specialized agents

`.github/agents/*.agent.md` — read the matching one before doing that kind of
work:

- [`slicer-engineer.agent.md`](../.github/agents/slicer-engineer.agent.md)
- [`docs-sync.agent.md`](../.github/agents/docs-sync.agent.md)
- [`threejs-3d-engineer.agent.md`](../.github/agents/threejs-3d-engineer.agent.md)
