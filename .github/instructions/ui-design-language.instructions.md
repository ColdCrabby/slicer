---
description: "Use for any UI/UX, styling, theming, component, or visual-polish work in the Angular front-end (ui/). Defines the Nexus Slicer design language: native-OS/Tauri feel, molten-amber accent tokens, no-blur rule, island/card patterns, and Angular styling gotchas. Read before touching ui/src styles, components, or theme tokens."
name: "Nexus Slicer UI Design Language"
applyTo: "ui/src/**"
---

# Nexus Slicer — Design Language & Philosophy

The `ui/` front-end targets a **Tauri desktop app that should feel native to the
host OS with Apple-level quality and finish**. It serves beginners and power
users at once: calm and obvious by default, dense and capable on demand. When in
doubt, choose the quieter, more restrained option.

## Guiding Philosophy

- **Native, not branded.** Stay as close as possible to the OS's native look.
  Inherit the user's **system accent color** at runtime; ship a **modern, subtle
  molten-amber** default. Never hardcode brand colors in components.
- **Modern & subtle.** Restraint over decoration. Prefer whitespace, surface
  tone shifts, and typography hierarchy to boxes, gradients, and heavy shadows.
- **Apple-quality finish.** Consistent spacing, aligned edges, no reflow/jitter,
  fixed-height chrome, purposeful motion. Details matter.
- **Beginners + power users.** Dashboards and big obvious actions for newcomers;
  keyboard shortcuts, dense settings, and full control for experts.

## Hard Rule: No Blur / Glass

**Never use `backdrop-filter`, `filter: blur()`, or frosted-glass surfaces.**
This does not fit the design language. Floating panels, notifications,
tooltips, cards, popovers, dialogs, and their enter/leave animations must use
**solid surfaces** (`--color-surface` / `--color-bg-secondary`). Give panels that
float _over the 3D scene_ definition with a border and/or shadow — but that is a
targeted exception, not the default (see Borders below). Animations may fade,
slide, or scale — never blur. Do not reintroduce `--backdrop-blur`-style tokens.

## Borders — Sparingly, for Contrast

**Borders are an emphasis tool, not a default.** Do not outline everyday chrome
and containers. Overusing hairline borders makes the UI look boxy and busy — the
opposite of the calm, native feel we want.

- **Separate with surface tone, not lines.** Prefer a background/surface shift
  (`--color-bg-primary` vs `--color-bg-secondary` vs `--color-surface`), spacing,
  or a subtle shadow to distinguish regions. The **sidebar, nav rail, panels, and
  structural chrome should NOT be boxed in with borders** — they read as part of
  the app shell via their surface tone.
- **Reserve borders for genuine contrast / attention:** inputs & form fields
  (so the hit target reads), focus rings, the active/selected item, cards that
  float over the 3D scene, menus/popovers detached from their surface, and
  emphasis states. If a border is not earning attention, remove it.
- When a divider is truly needed inside a calm region, prefer a single hairline
  `--color-border-light`/`--color-border-lighter` rather than boxing the element.

## Focus States — Border First

Keyboard focus should read as a **shape change**, not a fill change.

- **Default focus treatment:** animated **2px border/outline** on `:focus-visible`.
  Prefer an outer outline or an inner + outer 2px border effect over flooding the
  control with accent color.
- **Do not rely on accent fill alone** for focus indication. Fill can be used for
  selected/active/toggled states, but focus itself should remain clearly legible
  as a ring/border.
- **Animation:** keep focus transitions quick and purposeful (`--duration-fast`
  with `--ease-standard`), avoiding flashy pulses.
- **Special components may opt out** (3D canvas tools, custom gizmos, highly
  bespoke controls), but only when the replacement is equally clear and meets
  keyboard accessibility expectations.

## Design Tokens (single source of truth)

All values live in `ui/src/styles/theme/` — `_light.scss`, `_dark.scss`
(mode-specific), `_root.scss` (mode-independent + legacy aliases), `_tokens.scss`.
**Always consume CSS variables; never hardcode hex, px radii, or durations.**

- **Accent = single source of truth.** `--accent` (amber `#e0730f` light /
  `#f5883a` dark). Every shade — `--accent-hover/-active/-soft/-softer/-border/
  -contrast` — is derived via `color-mix(in oklab, ...)`. Overriding `--accent`
  alone (what `AccentService` does with the OS accent) recolors the whole UI.
  `--color-primary*` are legacy aliases of the accent — keep them aliased.
- **Neutrals:** warm **graphite** backgrounds/surfaces/text/borders
  (`--color-bg-*`, `--color-surface*`, `--color-text-*`, `--color-border*`).
- **Secondary/tertiary:** teal `--color-secondary*` (cool "go" balance), muted
  violet `--color-tertiary*` (sparse). Use sparingly.
- **Status:** `--color-success/warning/danger` (+ `-light`).
- **Focus:** `--color-focus-ring` (tracks accent) + `--color-focus-ring-glow`.
- **Spacing:** `--spacing-xs..2xl` (4/8/12/16/24/32). **Radius:** `--radius-sm/md/lg`
  (4/6/8). **Shadows:** `--shadow-xs..lg`. **Motion:** `--duration-fast/normal/slow`
  + `--ease-standard/decelerate/accelerate`. **Icons:** `--icon-stroke-width: 1.8`.

## Layout & Component Patterns

- **Islands / cards:** rounded solid surface, `--radius-lg`, `overflow: hidden`,
  separated from the app canvas (`--color-bg-primary`) by surface tone. Add a
  border only when the card needs contrast (floating over the 3D scene) rather
  than as a reflex.
- **Use the UI primitives** in `ui/src/app/ui/` — `button[nexusButton]`,
  `icon-button`, `nexus-section-header`, `empty-state` — instead of ad-hoc markup.
- **`nexus-section-header` has fixed `height: 48px; flex: none`** with nowrap
  title/desc — do not let it reflow.
- **Fixed-height chrome uses `flex: none`** (titlebar, section headers). A
  flex-shrinking titlebar/header collapsing is the classic bug here.
- **Settings pages** are left-aligned, `max-width: 720px`, no `margin: auto`;
  share the `_settings.scss` partial classes.
- **Accent usage:** primary/CTA buttons use `--accent` bg + `--accent-contrast`
  text; active/selected states use `--accent-soft`; ghost buttons on hover use
  `--color-surface-hover`.

## Angular Styling Gotchas (this project)

- **HMR does NOT cascade `@use`d SCSS partials.** After editing a theme partial,
  `touch` the consuming component `.scss` (or rebuild) or changes won't show.
- **Emulated encapsulation + router-injected hosts:** a `.parent > *` rule will
  NOT reach routed children (their host lacks the `_ngcontent` attr). Use
  `:host ::ng-deep` for shell/outlet layout rules that must reach routed hosts.
- **Avoid percentage-height children on `fr` grid tracks** (indefinite height) —
  use flex with `min-height: 0` instead.
- Standalone components, `ChangeDetectionStrategy.OnPush`, signal `input()`,
  `nexus-` selector prefix.

## Workflow

Per `ui-style-no-build.instructions.md`: for UI/style/polish work, **skip build
verification** — rely on the running dev server and browser inspection. Run
`pnpm exec prettier --write` on edited files. Only run `pnpm build` if the user
asks or for a major new feature.

## See also

- `ui/src/styles/theme/` — token definitions (`_light.scss`, `_dark.scss`, `_root.scss`)
- `ui/src/app/ui/` — shared primitives (button, icon-button, section-header, empty-state)
- `ui/src/app/services/accent.ts` — OS-accent inheritance (`AccentService`)
- `.github/instructions/ui-style-no-build.instructions.md` — build-verification policy
