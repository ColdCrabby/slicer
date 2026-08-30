---
description: "Use whenever writing or editing prose the user or a contributor reads — commit messages, PR titles/descriptions, CHANGELOG entries, README and module docs, code comments, doc comments, and UI copy. Do not reference GitHub issue or PR numbers (#123) unless the user explicitly asked for that specific reference."
name: "No Issue Numbers in Prose Unless Asked"
---

# No Issue / PR Numbers in Prose

**Describe the change, not its paperwork.** A reader of the code, the changelog,
or the UI cares *what* something does and *why* — not which tracker ticket it
came from. Issue and PR numbers are noise to them and rot the moment the repo
moves hosts or the numbering is renumbered.

## The rule

Do **not** write GitHub issue or PR references — `#123`, `issue #123`,
`(#123)`, `GH-123`, `fixes #123`, a bare `123` standing in for a ticket, or a
full `https://github.com/owner/repo/issues/123` URL — in any of:

- **CHANGELOG.md** entries
- **Commit messages** and **PR titles**
- **Markdown docs** — `README.md`, `AGENTS.md`, module `README.md`s, `RELEASING.md`, etc.
- **Code comments** and **doc comments** (`///`, `//`, `/* … */`, `#[doc]`, JSDoc)
- **UI copy** — anything a user reads on screen
- **Skills and instructions** files

Say what changed instead. Replace *"per-object identity (#22)"* with *"per-object
identity for exclude-object support"*; replace *"Fixes #487: skirt gap"* with
*"Fix the skirt gap so loops close toward the object"*.

## The only exceptions — the user asked

Include a number **only** when the user explicitly requests that specific
reference in this task. For example:

- "link the PR to issue 22", "add `Closes #22` so GitHub auto-closes it"
- "reference the tracking issue in the changelog"
- The `Closes #NN` / `Fixes #NN` line a repo requires in a **PR description** to
  trigger GitHub's auto-close — add it when opening a PR that resolves an issue,
  since that is a functional directive, not prose. Keep it to that one line; do
  not sprinkle the number through the rest of the description.

When in doubt, leave the number out — it is trivial to add on request and
tedious to scrub after the fact.


