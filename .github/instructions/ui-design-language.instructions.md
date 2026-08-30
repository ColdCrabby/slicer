---
description: "Use for any UI/UX, styling, theming, component, or visual-polish work in the Angular front-end (ui/). Defines the Nexus Slicer design language: native-OS/Tauri feel, molten-amber accent tokens, the single sanctioned backdrop-blur rule, island/card patterns, and Angular styling gotchas. Read before touching ui/src styles, components, or theme tokens."
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

## Blur / Glass — One Sanctioned Effect Only

There is **exactly one** backdrop blur in the whole app, and it is a token:
`backdrop-filter: var(--backdrop-blur)` (defined once in `_root.scss`). Any
frosted surface **must** consume that token — never hand-roll a `blur()` amount,
never stack a second, different blur, and never `filter: blur()` a whole element.

- **Where it is allowed:** a semi-transparent surface that floats _over the 3D
  scene or live content_ — the drag-and-drop drop card, the slice launch card,
  and the viewport-cube roll buttons. Blur only reads when the surface is
  translucent, so pair `var(--backdrop-blur)` with a `color-mix(... transparent)`
  fill (or `--backdrop-bg`) plus a border/shadow for definition.
- **Where it is still forbidden:** everyday chrome, notifications, tooltips,
  popovers, dialogs, menus, and any opaque panel. These stay on **solid
  surfaces** (`--color-surface` / `--color-bg-secondary`). A solid surface gains
  nothing from blur — do not add it.
- **Enter/leave animations still never blur.** Animate opacity, transform
  (slide/scale), or the surface fill — never animate the blur radius.
- **Do not add a second blur token or vary the radius per component.** The
  single `--backdrop-blur` value is what keeps the effect consistent.

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

## Destructive Actions — Confirm by Impact

Potentially harmful actions (especially delete) are a classic source of user
error and must be explicitly double-checked before execution.

- **Default pattern for routine destructive actions:** prefer an inline, in-place
  two-step confirm instead of a blocking modal.
- First click: do not execute. Transition the control to a destructive
  confirmation state (for example, trash icon/button turns danger red and label
  changes to `Confirm?`).
- Second click on that same control: execute the action.
- Keep this confirmation state obvious but temporary; if focus is lost or a
  short timeout passes, reset back to the safe default state.
- Use clear destructive language (`Delete`, `Remove`, `Confirm?`) and danger
  tokens (`--color-danger`, `--color-danger-light`) for the confirm state.

For high-impact or irreversible data loss (for example deleting a printer),
inline double-click confirmation is not enough.

- Require a **typed confirmation challenge** with known item identity.
- Ask the user to enter a specific value tied to the target item (for example,
  the exact printer name) before enabling final delete.
- Match exactly and clearly show what must be typed.
- Keep the default action safe (`Cancel`/close), and style the final destructive
  submit as danger.

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
  - `--ease-standard/decelerate/accelerate`. **Icons:** `--icon-stroke-width: 1.8`.

## Layout & Component Patterns

- **Islands / cards:** rounded solid surface, `--radius-lg`, `overflow: hidden`,
  separated from the app canvas (`--color-bg-primary`) by surface tone. Add a
  border only when the card needs contrast (floating over the 3D scene) rather
  than as a reflex.
- **Use the UI primitives** in `ui/src/app/ui/` — `button[nexusButton]`,
  `icon-button`, `nexus-section-header`, `empty-state`, `inline-notice` — instead
  of ad-hoc markup.
- **`nexus-section-header` has fixed `height: 48px; flex: none`** with nowrap
  title/desc — do not let it reflow.
- **Fixed-height chrome uses `flex: none`** (titlebar, section headers). A
  flex-shrinking titlebar/header collapsing is the classic bug here.
- **Settings pages** are left-aligned, `max-width: 720px`, no `margin: auto`;
  share the `_settings.scss` partial classes.
- **Accent usage:** primary/CTA buttons use `--accent` bg + `--accent-contrast`
  text; active/selected states use `--accent-soft`; ghost buttons on hover use
  `--color-surface-hover`.

## Contextual Notices & Cautions

When a control needs a note, hint, or caution, use the **`nexus-inline-notice`**
primitive — never hand-roll a coloured box. It is a solid **tinted surface (no
blur)** with a `tone` input (`info` teal / `warning` amber / `danger` red) that
drives its icon and border via one `--notice-tone` custom property.

Follow the **"detail at the source, neutral hint on the container"** pattern for
surfacing these (canonical: the schema-form settings sidebar):

- **Put the tone-coloured detail right next to the offending control**, not at
  the bottom of a section. The reader should see _which_ setting the caution is
  about without hunting.
- **Aggregate a single colour-_neutral_ cue on the collapsible container** (an
  accordion header, a tab) whenever anything inside currently has a notice —
  a `warning-triangle` held at `--color-text-tertiary`, so a collapsed section
  still says "look inside." Keep it neutral: it is a calm "double-check" nudge,
  **not** a second alarm competing with the tone-coloured detail.
- **Drive the container cue from the same source as the detail** (a `computed`
  over the same predicate), so the hint and the note can never drift apart.
- Notices are **conditional on live state** — show them only while the risky
  value is actually selected (e.g. raft adhesion), and let them disappear
  reactively when it changes.

For schema-driven settings specifically, declare the caution in the
**field-exceptions registry** rather than special-casing the generic form — see
the component-structure instruction's "Exceptions beside a generic resolver."

### Cross-contract dependencies — say so, and link to the fix

Settings are split across three profile **contracts** (Printer / Filament /
Process — see `models/setting-contract.ts`). A setting in one contract regularly
depends on a setting in **another**: the filament asks for a heated chamber, the
printer is what has one; sequential printing needs the machine's extruder
clearances. The user sees only the tab they are on, so the dependency is
invisible right up until the feature quietly does nothing.

**Be transparent about it. A setting that cannot take effect must say so, where
it is set, while it is set.** Silence is the worst option: a chamber temperature
that is never emitted looks identical to one that is — until the print warps.

- **Warn at the dependent setting, not at the prerequisite.** The user is
  looking at the filament's chamber temperature; that is where the note belongs.
  The printer's own control has nothing to apologise for.
- **State the actual consequence in plain words** — "no chamber command will be
  emitted", not "this setting may be ignored." Say what the slicer will *do*.
- **Link to where the prerequisite is configured** via `FieldNotice.link`
  (`{ text, routerLink }`). Most of the time the user simply has not set it yet,
  and the honest response is a one-click path to fixing it rather than a
  dead-end complaint. Name the destination in the link text ("Enable it in
  printer settings"), never "click here."
- **Distinguish "misconfigured" from "deliberately off."** Only warn when the
  intent is real and unmet — a chamber temperature of `0` is not a mistake, so
  it gets no notice. `tone: 'warning'` for "you asked for something you will not
  get"; `tone: 'info'` for a consequence worth knowing that is not a mistake.
- **Mirror it in the engine.** The UI is not the only front end. If a setting
  can be silently inert, `SlicingParams::unsupported_feature_warnings` should say
  so too, so the CLI and the WS log are equally honest.

Evaluating a cross-contract condition needs sibling values, which is why
`FieldException.notice` receives the whole values record and why the profile
editors pass the **active printer's** params alongside the profile being edited.

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
- `ui/src/app/ui/` — shared primitives (button, icon-button, section-header, empty-state, inline-notice)
- `ui/src/app/ui/inline-notice/inline-notice.ts` — the contextual-notice primitive
- `ui/src/app/schema-form/` — the "detail at the source, neutral hint on the container" pattern in practice
- `ui/src/app/services/accent.ts` — OS-accent inheritance (`AccentService`)
- `.github/instructions/ui-style-no-build.instructions.md` — build-verification policy
