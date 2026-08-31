# Brand

Cold Crabby is a crab hugging an ice cube. That's most of what you need to know.

<img src="https://raw.githubusercontent.com/ColdCrabby/slicer/main/ui/public/logo_hero.png" alt="Cold Crabby — a crab hugging an ice cube" width="180" />

## The name

**Cold Crabby.** Two words, both capitalised. Never "ColdCrabby", never
"coldcrabby", never abbreviated to "CC".

The desktop application is **Cold Crabby Desktop**. The underlying Rust project
is **slicer-engine**, which is what you'll see in repository names, binaries and
config paths — use it when you mean the engine, not the product.

## The mascot

A crab hugging an ice cube. Warm creature, cold object — which is the joke, and
also the point: a slicer is a friendly tool doing something precise.

The mascot leads. Where a logo appears at any reasonable size, it's the crab,
not a wordmark. The wordmark sits beside it in the app chrome and works alone
only where the mascot would be illegible.

## Assets

All in [`ui/public/`](https://github.com/max-scopp/slicer-engine/tree/main/ui/public).

| File | Use |
| --- | --- |
| `logo_hero.png` | README headers, marketing, anywhere large |
| `logo.png`, `logo@2x.png`, `logo@3x.png` | The animated mark |
| `logo_still.png`, `logo_still@2x.png`, `logo_still@3x.png` | The static mark, used in the app's own chrome |
| `logo_source.png` | The 1024² master everything else is cut from |
| `splash-logo.webp` | The boot splash's mark — generated, see below |
| `apple-touch-icon.png`, `favicon*` | Web and home-screen icons |

**Application icons are generated, never hand-edited.** One master —
`ui-desktop/src-tauri/app-icon.png`, cropped from `logo_source.png` — feeds every
platform via `pnpm run icons`. If an icon looks wrong, fix the master and
regenerate.

**The boot splash's logo is generated too**, from `logo_still@3x.png` via
`pnpm run splash-logo` (needs `brew install webp`). It emits two things: the
`splash-logo.webp` asset, and a tiny base64 stand-in embedded directly in
`ui/src/index.html` that shows before any request completes. Never hand-edit
that blob — change the artwork and regenerate.

## Colour

The default accent is **molten amber**.

| | Light | Dark |
| --- | --- | --- |
| Accent | `#e0730f` | `#f5883a` |

Five alternates ship alongside it, and users can pick any colour they like:

| | |
| --- | --- |
| Teal | `#0d8f86` |
| Indigo | `#5b62e0` |
| Violet | `#7c5cff` |
| Rose | `#e0568b` |
| Forest | `#3f9d5a` |

On macOS and Windows the app can inherit the **system accent** instead. That's
not a compromise — it's the intent. See below.

Everything else is neutral: near-white surfaces in light mode, deep graphite in
dark. Models render in neutral grey unless you turn on filament colouring.

## The design language

**Native, not branded.** The interface should look like it belongs to the
operating system it's running on, not like a website that happens to be in a
window. Brand colour lives in the accent and the mascot, and nowhere else. No
component hardcodes a brand colour.

**Modern and subtle.** Whitespace, surface tone and typography do the work.
Gradients, heavy shadows and decorative boxes don't. When in doubt, the quieter
option is correct.

**Beginners and power users at once.** Big obvious actions and sensible defaults
for someone's first print; keyboard shortcuts, dense settings and full control
for someone's thousandth. Not two modes — one interface that reveals depth as
you look for it.

**Restraint has rules.** There is exactly one blur effect in the whole app, used
only where a surface floats over the 3D scene. Borders are an emphasis tool, not
a default — regions separate by surface tone, not by lines. Focus reads as a
shape change, not a colour flood.

Full detail is in
[the design language guide](https://github.com/max-scopp/slicer-engine/blob/main/.github/instructions/ui-design-language.instructions.md),
which is what contributors work to.

## Voice

Plain, direct, and honest about limits.

- Say what something does, not how sophisticated it is.
- Name the trade-off. "Smaller means smoother and slower" beats "optimised
  quality".
- Warn rather than refuse, where a check is an estimate rather than a fact.
- Errors carry the actual reason. A bad API key and an unreachable host should
  never read the same.
- No exclamation marks, no "simply", no "just".

## Using the brand

The name, mascot and logo belong to the project. Using them for your own product
or implying an endorsement isn't permitted.

Writing about Cold Crabby, linking to it, screenshotting it, or using the logo to
refer to the project itself is fine and welcome.

The project's code is **all rights reserved** pending a licence decision. See
[Data, privacy and licensing](/teams/data).
