---
description: "Use when creating, splitting, refactoring, or reviewing Angular components in the ui/ front-end. Covers when to split vs. keep whole (don't split too early, don't grow god-files), the smart (container) vs. dumb (presentational) distinction, and how to keep components composable and reusable without bloated input/output contracts."
name: "Angular Component Structure & Composition"
applyTo: "ui/src/app/**"
---

# Angular Component Structure & Composition

How to decide when a component should be split, how to separate concerns
cleanly, and how to keep components small, composable, and reusable. These rules
sit **on top of** the design-language instruction — this file is about component
_shape and boundaries_, not styling.

Read `ui/src/app/ui/button/button.ts` and any `pages/*/*.ts` as canonical
examples of dumb vs. smart components.

## The One Rule

**A component owns exactly one job.** Either it _decides_ (fetches, coordinates,
holds state) or it _presents_ (renders inputs, emits events). When a single
component starts doing both at meaningful scale, that is the signal to split —
not before.

## Smart (container) vs. Dumb (presentational)

|                        | Smart / container                        | Dumb / presentational                  |
| ---------------------- | ---------------------------------------- | -------------------------------------- |
| Lives in               | `pages/`, feature roots in `components/` | `ui/`, `shared/`, leaf `components/`   |
| Knows about            | services, routing, WASM, state           | nothing but its `input()`s             |
| Gets data via          | `inject()`ing services/signals           | `input()` only                         |
| Talks back via         | calling service methods                  | `output()` only                        |
| Reused across features | rarely                                   | freely                                 |
| Selector               | element `nexus-*`                        | attribute (`[nexusButton]`) or element |

- **Dumb components should be pure functions of their inputs.** As a rule they
  don't `inject()` app services, HTTP, the router, or `NotificationService`. If
  one is genuinely needed, that's a strong sign the component is really smart —
  move it up rather than reaching down.
- **Smart components hold the wiring, delegate the pixels.** A page injects
  services, derives signals, and renders dumb children. It should contain little
  to no visual markup of its own beyond layout.
- **Push state up, push presentation down.** When a dumb component reaches for
  global state, lift that need into its smart parent and pass it as an input.

```ts
// dumb: pure, reusable, testable without a TestBed harness
@Component({
  selector: "nexus-slice-summary",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SliceSummary {
  readonly stats = input.required<SliceStats>();
  readonly rerun = output<void>();
}

// smart: owns the service, feeds the dumb child
@Component({ selector: "nexus-slice-viewer" /* ... */ })
export class SliceViewer {
  readonly #slicer = inject(SlicerService);
  readonly stats = this.#slicer.stats; // signal
  rerun() {
    this.#slicer.rerun();
  }
}
```

## When to Split — and When NOT to

**Don't split too early.** A single well-named component that reads top-to-bottom
is better than three files you must jump between. Speculative "might reuse this"
extraction creates indirection with no payoff. Extract on the _second_ real use,
not the first imagined one.

**Do split when any of these are true:**

- The template has a **clearly independent visual block** that could stand alone
  (a card body, a toolbar, a row) and is repeated or conditionally swapped.
- The class mixes **decision logic and presentation** that each pull in their own
  dependencies (e.g. a service _and_ heavy DOM/animation code).
- A file is **hard to hold in your head** — you can't scan the class or template
  without repeatedly scrolling and losing the thread.
- Two parts **change for different reasons** (different owners, different release
  cadence) — the classic single-responsibility seam.

**Don't split just because a file is long** if it is one cohesive concern with no
seam. Length is a _prompt to look_ for a seam, not a seam by itself. Prefer
extracting a **dumb child** or a **service/helper** over shattering one idea into
fragments that only make sense together.

## Composable & Reusable — Avoid Contract Overload

Reusability comes from a **small, honest contract**, not from options.

- **Cap the surface area.** When a dumb component keeps accreting `input()`s or
  several `boolean` mode flags, it is trying to be several components. Split by
  variant, or accept **projected content** (`<ng-content>` / `<ng-template>`)
  instead of a config prop per slot.
- **No boolean-flag soup.** `showX`, `isCompact`, `variantB`, `hideFooter`
  interacting combinatorially = a god-component. Prefer a `variant` enum, or
  separate components that share a primitive.
- **Content projection over configuration** for anything structural. Let callers
  compose the inside (`<nexus-card><h2>…</h2>…</nexus-card>`) rather than passing
  `title`, `subtitle`, `icon`, `actions[]` as parallel inputs.
- **One reason to emit.** Keep `output()`s semantic (`dismiss`, `rerun`), not
  low-level (`buttonClicked`, `keyPressed`) — the parent shouldn't reverse-engineer
  intent from raw events.
- **Depend on data shapes, not app concepts.** A reusable component takes a
  `SliceStats` value, never reaches into `SlicerService`. That is what makes it
  drop-in across features.

## Exceptions beside a generic resolver

When a **generic, data-driven renderer** (a schema form, a dynamic table, a
component-outlet host) needs a handful of items treated specially, do **not**
branch inside the generic path — that is how a clean resolver rots into a pile of
`if (key === …)`. Instead keep the generic path clean and add a **parallel
registry keyed by item id** for the exceptions.

Canonical example — the schema-form has **two independent maps**:

- `field-registry.ts` — `key → widget` ("_which_ control renders this field").
- `field-exceptions.ts` — `key → exception` ("does this field need extra
  treatment", e.g. a conditional caution notice).

The widget-choosing never learns about notices, and vice-versa; each map stays a
one-liner per entry. Guidelines:

- **Render the exception at the point of use** (inside the per-item host), so it
  works everywhere that item renders — grouped view, search results, anywhere —
  with zero duplication.
- **Drive any container-level aggregate from the same registry** via a
  `computed` (e.g. an accordion header that flags "something inside needs a look"
  reuses the exceptions predicate), so the summary and the detail can't drift.
- **Shape the exception for extension, not just today's need.** An exception
  object with an optional `notice?()` leaves room for future kinds (badge, hidden,
  disabled) without reshaping call sites — but add those only when a real second
  case appears.

## Conventions (match the existing codebase)

- **Standalone, `ChangeDetectionStrategy.OnPush`, signals throughout.** Use
  `input()` / `input.required()` / `output()` / `computed()` / `signal()`; do
  **not** use `@Input()` / `@Output()` decorators or `EventEmitter`.
- **`inject()` into `readonly #field`s** for dependencies (private JS fields), not
  constructor parameters.
- **Selectors:** `nexus-*` element selectors for feature/leaf components;
  attribute selectors (`selector: 'button[nexusButton]'`) for design-system
  primitives applied to native elements.
- **Placement:** `ui/` = presentational design-system primitives (barrel-exported
  from `ui/index.ts`); `shared/` = cross-feature reusable pieces; `components/` =
  feature components; `pages/` = smart route-level containers.
- **File split:** keep `*.ts` / `*.html` / `*.scss` as separate files (inline
  `template:` only for trivial primitives like `Button`). When you add a new
  reusable primitive under `ui/`, export it from `ui/index.ts`.

## Non-goals

- Not a mandate to extract every block into its own file — cohesion beats
  fragmentation.
- Not a rule to make everything reusable — most components are single-use; that's
  fine. Generalise only when a second caller actually appears.
- Not about styling, tokens, or theming — see the design-language instruction.

## See also

- `.github/instructions/ui-design-language.instructions.md` — visual language.
- `ui/src/app/ui/button/button.ts` — canonical dumb primitive.
- `ui/src/app/pages/slice-viewer/slice-viewer.ts` — canonical smart container.
- `ui/src/app/schema-form/field-registry/field-registry.ts` + `field-exceptions/field-exceptions.ts` — the "exceptions beside a generic resolver" pattern.
