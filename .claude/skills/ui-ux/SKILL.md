---
name: ui-ux
description: Read the project's UI/UX rules before any front-end work in ui/ — styling, theming, tokens, component structure, surfacing a setting, or visual polish. Use when the user says "UI", "UX", "design", "style", "theme", "component", "layout", "settings panel", "make it look…", "polish", or when a task touches ui/src at all. Covers the Nexus design language, progressive disclosure, Angular component boundaries, and the skip-the-build workflow.
---

# UI/UX Work in `ui/`

The rules live in `.github/instructions/` — **one copy**, shared with GitHub
Copilot, which applies them automatically via `applyTo`. This skill is how you
find the right one. **Read the matching file before editing**; do not improvise a
rule this project has already decided.

## Which file

| Working on | Read |
| --- | --- |
| Colours, tokens, spacing, blur, borders, focus, cards, notices, theming | [`ui-design-language`](../../../.github/instructions/ui-design-language.instructions.md) |
| Exposing a setting or capability — naming it, tiering it, help text, `Automatic`, honesty about impact | [`progressive-disclosure`](../../../.github/instructions/progressive-disclosure.instructions.md) |
| Splitting, creating or reviewing a component; smart vs. dumb; input/output contracts | [`angular-component-structure`](../../../.github/instructions/angular-component-structure.instructions.md) |
| Whether to run a build to verify | [`ui-style-no-build`](../../../.github/instructions/ui-style-no-build.instructions.md) |

Most non-trivial UI tasks touch **two** of these — a new settings control is
both *design language* (how the notice looks) and *progressive disclosure*
(whether it should be visible at all). Read both rather than guessing which one
governs.

## The through-line

The app targets a **Tauri desktop app that feels native to the host OS, with
Apple-level finish**, and serves beginners and power users at once: calm and
obvious by default, dense and capable on demand.

> **Simple by default. Complete by design.** Every capability exists; the
> interface only asks the user to think about one when it matters.

When in doubt, choose the quieter, more restrained option.

## Cheap rules worth knowing before you open anything

These are the ones most often broken. Each is explained in the file it comes
from — go there for the reasoning.

- **Never hardcode a colour, radius, duration or font.** Consume the CSS
  variables. `--accent` is the single source of truth and is inherited from the
  OS accent at runtime.
- **There is exactly one backdrop blur** (`var(--backdrop-blur)`), and it is only
  for translucent surfaces floating over the 3D scene. Never on opaque chrome,
  tooltips, dialogs or menus.
- **Borders are an emphasis tool, not a default.** Separate regions with surface
  tone, not hairlines. The sidebar, nav rail and panels are not boxed in.
- **Focus is a shape change** — a 2px `:focus-visible` outline — not an accent
  fill.
- **Destructive actions confirm by impact**: inline two-step confirm for routine
  ones, a typed challenge for irreversible data loss.
- **Use the primitives** in `ui/src/app/ui/` and `@coldcrabby/ui` rather than
  ad-hoc markup — especially `nexus-inline-notice` for any hint or caution.
- **Icon-only controls need an `aria-label`.** The `tooltip` directive
  contributes no accessible name, and a tablet has no hover to reveal it.
- **Phones, tablets and touchscreens are three separate questions**, asked
  independently. Never an ad-hoc `max-width`. See
  [`ui/README.md`](../../../ui/README.md#phones-and-tablets).

## Workflow

**Skip build verification for UI, style and polish work** — rely on the running
dev server and browser inspection. Run `pnpm exec prettier --write` on the files
you edited. Only run `pnpm build` if the user asks, or for a major new feature.

If you need the app running, `pnpm run dev` serves it on a **seeded** port pair
and prints the URL — report that URL, never a hardcoded one.
