---
description: "Use when exposing any setting, option, or capability in the UI — adding a slicing parameter, designing a settings section, naming a control, writing help text, or deciding whether something belongs in the main view at all. Defines the tier model (Everyday / Advanced / Expert), Automatic as a first-class value, and the honesty rules for describing what a knob actually does."
name: "Progressive Disclosure — Simple by Default, Complete by Design"
applyTo: "ui/src/**, src/settings/**"
---

# Progressive Disclosure

> **Simple by default. Complete by design.**

The slicer serves a beginner printing their first Benchy and someone who will
spend Saturday night changing a 0.01 mm tolerance to see what happens. Both are
right, and the interface has to hold both without insulting either.

**This is not a "basic vs. advanced user" problem.** An expert does not want
*more UI* — they want more control *when they go looking for it*. Treating it as
a user-type problem produces two bad outcomes at once: a beginner mode that hides
capability, and an expert mode that is a wall of 400 settings.

So the rule is:

> **Every capability exists. The interface only asks the user to think about one
> when it matters.**

---

## 1. Expose the outcome, not the implementation

A control is named for what it achieves, not for the algorithm behind it.

| Don't | Do |
| --- | --- |
| `Mesh quality: 0.15 mm` | **Surface quality** — Balanced · Fine · Ultra |
| `Arc tolerance: 0.05` | folded into the same quality choice |
| `Arachne transition threshold: 0.42` | not surfaced at all outside Expert |

Most users never need to know what mesh refinement *does*. They need to know
whether the result will look good.

**This does not mean deleting the raw value** — see *Automatic is a first-class
value* below. It means the raw value is not the thing the main view asks about.

---

## 2. Three tiers — and never call two of them "beginner" and "advanced"

| Tier | Holds | Example |
| --- | --- | --- |
| **Everyday** | The decisions a print actually depends on | Layer height · Infill · Walls · Supports · Speed · Material · Quality |
| **Advanced** | Real choices a user can reason about | Seam strategy · wall ordering · infill pattern detail · support thresholds · acceleration · cooling behaviour · mesh refinement |
| **Expert** | Algorithm knobs, tolerances, heuristics, experimental and debug controls | Bead transition thresholds · medial-axis prune lengths · internal epsilons |

Two rules about the labels themselves:

- **The default view is not "easy mode".** A professional must be able to work
  entirely in Everyday without feeling they have been handed the toy version.
  Never label the first tier — it is simply *the settings*. "Advanced" is a
  disclosure affordance, not a badge of user competence.
- **The Advanced/Expert split is about reasoning, not difficulty.** Seam
  position is *advanced* because a user can form an intention about it ("keep
  the seam at the back"). A bead-transition threshold is *expert* because almost
  nobody — including experienced users — can predict what changing it does.
  When in doubt, ask: **can the user reason about this, or only poke it?**

---

## 3. Advanced is an expansion, not a destination

Clicking **Advanced** expands the section in place. It does **not** navigate
somewhere else, open a separate "expert mode", or switch the app into a
different shape.

```
Quality
────────────────────────────
Layer height        0.20 mm
Wall quality        High
Surface quality     Balanced

              Advanced ⌄
```

The user keeps their place, their context, and their scroll position. A mode
switch loses all three and makes the setting they wanted feel like it lives in a
different application.

Corollary: **the UI is quiet until the user demonstrates intent.** Do not
permanently render the expanded state "so it is discoverable" — the chevron is
the discovery.

---

## 4. Automatic is a first-class value

Wherever the engine can choose sensibly, **`Automatic` is a real value in the
enum**, not the absence of a choice:

```
Geometry quality
    ● Automatic
      Fine
      Very fine
      Custom          →  Mesh tolerance    0.05 mm
                         Minimum segment   0.02 mm
```

This is the single highest-leverage pattern in this document, because it lets
the algorithms evolve without the UI having to grow a control for every new
heuristic. A new refinement pass added behind `Automatic` ships to every user
who never touched the setting, and changes nothing for the one who did.

- **`Custom` is the escape hatch**, and it reveals the raw values — it does not
  merely unlock them somewhere else.
- **Never model `Automatic` as a magic sentinel the user can type** (`-1`, `0`
  meaning "auto"). If the engine needs a sentinel internally, the *UI* still
  shows a named choice. The one exception is where a sentinel is already the
  established convention in a field imported from other slicers, and then the
  label must say what it means.

---

## 5. Be honest about what a setting does

A setting that exists mainly for discoverability, perceived control, or
familiarity with terminology is **legitimate** — it makes the slicer feel
technically credible and gives the tinkerer their knob. What is not legitimate
is implying it matters more than it does.

```
Mesh refinement                                   ⓘ
0.10 mm
Controls how closely curved geometry is approximated.
Usually has little effect on print quality.
```

That last line is the whole point. Compare the alternatives:

- Hiding the setting → the user who wants it thinks the slicer cannot do it.
- Showing it with an important-sounding description → the user burns an evening
  tuning something inert, and trusts the next description less.
- Showing it with an honest one → everybody gets what they came for.

**Best of all is a notice that knows the current model:**

```
Mesh refinement
0.10 mm
ⓘ No significant effect detected for this model.
```

That teaches the user what actually matters, which is the most valuable thing a
slicer's settings UI can do.

**Mirror honesty in the engine.** The UI is not the only front end. If a setting
can be silently inert, `SlicingParams::unsupported_feature_warnings` should say
so too, so the CLI and the WS log are equally honest.

---

## 6. Explain contextually, never permanently

Do not line the panel with paragraphs. Attach the explanation to the control and
reveal it on hover, focus or tap (`ⓘ`). One or two sentences: what it controls,
and what moving it trades away.

Where a caution depends on **live state** — a risky value currently selected, a
prerequisite unmet — use the `nexus-inline-notice` primitive and the
"detail at the source, neutral hint on the container" pattern from
[`ui-design-language`](ui-design-language.instructions.md). A notice appears
while the condition holds and disappears when it stops.

---

## 7. Search is the real escape hatch

For a slicer, **a good search matters more than exposing everything in the
layout**. Someone who knows the term can type it and land on the control,
wherever it lives in the taxonomy — which is what makes it safe to keep the
default view calm.

This already exists: [`schema-form.ts`](../../ui/src/app/schema-form/schema-form.ts)
runs a Fuse.js fuzzy search over every field, and it deliberately **spans all
contracts** even when the panel is scoped to one tab. Two consequences:

- **A setting must be findable by the words a user would actually type**,
  including the name other slicers use for it. The schema `title` and
  `description` are the search corpus — write them for a person, not as a
  restatement of the field name.
- **Search must reach Expert-tier settings too.** Tiers govern what is *shown by
  default*, never what *exists*. A tier that hides a setting from search has
  become a feature flag, which is not what this is for.

---

## 8. Dropping a control is a legitimate answer; hiding a needed one is not

On a phone the viewport cube and the pipeline inspector are **removed**, because
a drag gizmo has no touch equivalent and eleven pill buttons do not fit. Nothing
on the path to a slice is ever removed. The same judgement applies to tiers:
demote a control the user cannot reason about, never one they need to finish the
job.

---

## How this maps onto the schema

The settings UI is **fully schema-driven**: a new `SlicingParams` field with an
`x-group` appears in the UI with no TypeScript change at all, and flows through
`cache_fingerprint` and profile resolution automatically, since both walk the
serialized struct by name.

**What exists today:**

| Mechanism | Where | Does |
| --- | --- | --- |
| `x-group` | `src/settings/params.rs` | Puts the field in an accordion group |
| `x-relevant-when` | same | Hides a field until a sibling makes it meaningful — `equals` for a switch, `greaterThan` for a numeric feature that is off at `0` |
| `x-tier` | same | Everyday (omit) / `advanced` / `expert` — what the panel shows before the user asks |
| `x-widget` | same | Overrides the control chosen from the field's shape |
| `SETTING_CONTRACTS` | [`setting-contract.ts`](../../ui/src/app/models/setting-contract.ts) | Assigns each group to the Printer / Filament / Process tab |
| `GROUP_ICONS` | same | The group's icon |
| Field exceptions | [`field-exceptions.ts`](../../ui/src/app/schema-form/field-exceptions/field-exceptions.ts) | Conditional `FieldNotice`, including a `link` to a prerequisite on another tab |
| Fuzzy search | [`schema-form.ts`](../../ui/src/app/schema-form/schema-form.ts) | Fuse.js over every field |
| Collapsible groups | same | Expand state persisted per group |

**`x-relevant-when` is progressive disclosure already in force** — it is how the
elephant-foot taper stays out of sight while the compensation is `0`, and how
classic-only wall options vanish under Arachne. Reach for it before inventing
anything new. Evaluation lives in exactly one place,
[`relevance.ts`](../../ui/src/app/schema-form/models/relevance.ts), shared by the
settings panel and the profile editor pages, so every schema-driven surface hides
the same fields.

**`x-tier` implements the three tiers above.** Omit it for Everyday; set
`"advanced"` or `"expert"` for the rest. It is evaluated in
[`relevance.ts`](../../ui/src/app/schema-form/models/relevance.ts) beside
`x-relevant-when`, because the two answer the same shape of question — *should
this be on screen right now?* Relevance is about the state of the plate; tier is
about how far the user has asked to look.

```rust
#[schemars(
    description = "…",
    extend("x-group" = "Walls", "x-tier" = "expert")
)]
pub wall_transition_threshold: f64,
```

Each accordion group renders its Everyday fields, then a quiet
`Advanced ⌄ 10` footer that expands **in place** — the count is what makes it
worth pressing, since a bare chevron says only that something is there. A second
press reveals Expert. The step is offered only when the group can actually fill
it, so a section whose extra fields are all Advanced never advertises an Expert
tier that would expand to nothing.

Three rules the implementation depends on — each has a test in
[`tier.spec.ts`](../../ui/src/app/schema-form/models/tier.spec.ts):

- **Search is never tier-filtered.** `flatFields` builds the Fuse index from the
  untiered `relevantGroups`, and must keep doing so. This is what makes a calm
  default view affordable, and a tier that hid a setting from search would have
  stopped being disclosure and become a feature flag.
- **A modified field is always shown, whatever its tier.** Hiding a value the
  user has already changed is the one failure that cannot be argued for: they
  cannot put it back if they cannot find it, and the group header's "changed"
  dot would point into an empty section.
- **The Everyday set has a ceiling.** If it grows without anyone noticing, the
  panel is a wall of settings again and the tiers have stopped working, so the
  size is asserted rather than assumed.

Reveal state is persisted per group, the same way the accordion's own expansion
is: someone who works in Advanced all day should not reopen it every session.
That is not the "I am an expert" switch the non-goals rule out — it is per
section, per intent, and it never changes the shape of the app.

### Adding a setting

Add the field with an `x-group` and it appears. Regenerate the schema
(`pnpm run gen-schemas`; it is git-ignored). Write the `title` and `description`
as search corpus and as the contextual explanation — that is what §5 and §7 are
made of.

### Adding a *group* needs two more lines

A new group must be claimed by a contract in `SETTING_CONTRACTS` and given an
entry in `GROUP_ICONS`. Otherwise it falls to the end of Process, out of taxonomy
order, with a blank where its icon should be.

`setting-contract.spec.ts` reads the generated schema and **fails on any
unclaimed or iconless group**, so this cannot regress silently — it is what
caught `Time estimate`, which had been unclaimed since it was added.

### Cross-contract dependencies

A setting in one contract regularly depends on one in another: the filament asks
for a heated chamber; the printer is what has one. The user sees only the tab
they are on, so the dependency is invisible right up until the feature quietly
does nothing. That is a disclosure failure, and the rules for it are in
[`ui-design-language`](ui-design-language.instructions.md#cross-contract-dependencies--say-so-and-link-to-the-fix).

---

## Non-goals

- **Not a permissions model.** Tiers change what is *shown by default*. They
  never gate capability, and they never hide a setting from search.
- **Not a per-user preference.** There is no "I am an expert" switch to flip
  once; disclosure is per-section and per-intent.
- **Not a reason to add settings.** "It can live in Expert" is not a
  justification for a knob nobody can reason about. A defensible Expert setting
  is one a real person has wanted to change.
- **Not a substitute for good defaults.** If a setting must be tuned for a normal
  print to succeed, the default is wrong. Fix the default.

---

## See also

- [`ui-design-language.instructions.md`](ui-design-language.instructions.md) — the
  visual language these controls are built from, and the notice patterns
- [`angular-component-structure.instructions.md`](angular-component-structure.instructions.md) —
  component shape and boundaries
- [`src/settings/README.md`](../../src/settings/README.md) — the parameter reference
- [`ui/README.md`](../../ui/README.md#phones-and-tablets) — how the same
  "show what is needed" judgement plays out across viewports
