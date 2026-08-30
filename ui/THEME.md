# Theming

How light/dark mode and the accent colour work. For the SCSS file layout, see
[`src/styles/README.md`](src/styles/README.md); for the visual rules that govern
what you build with these tokens, see
[the design language](../.github/instructions/ui-design-language.instructions.md).

## Modes

Dark mode is a class on `<html>`. Light is the default; `html.dark` swaps the
token values.

Tokens are defined per mode in [`src/styles/theme/`](src/styles/theme/):
`_tokens.scss` holds what doesn't change (spacing, radii, durations),
`_light.scss` and `_dark.scss` hold the colours.

[`AppTheme`](src/app/services/app-theme.ts) owns the switch:

| | |
| --- | --- |
| `isDarkMode()` | The resolved mode, as a signal |
| `hasExplicitPreference()` | False when following the system |
| `toggleTheme()` | Flip and persist |

With no stored preference it follows `prefers-color-scheme` and keeps following
it live. That's why there are three options in the UI — Light, Dark and System —
but only two token sets.

## Accent

**Every colour in the UI derives from one `--accent` variable.** Override it and
the whole interface recolours; nothing else needs to know.

[`AccentService`](src/app/services/accent.ts) resolves where that value comes
from:

| Source | Value |
| --- | --- |
| `brand` | The molten-amber default baked into the theme tokens |
| `system` | The OS accent colour (desktop only) |
| `custom` | A preset or a colour the user picked |

It defaults to the OS accent on desktop and to brand on the web — the app should
look like it belongs to the machine it's running on.

## Using tokens

In component styles, use the CSS variables. Don't reach for SCSS colour
functions, and never hardcode a brand colour:

```scss
.my-element {
  background: var(--color-bg-secondary);
  color: var(--color-text-primary);
  padding: var(--spacing-md);
  border-radius: var(--radius-md);
  transition: all var(--transition-normal);
}
```

Hardcoding a colour breaks three things at once: dark mode, the user's accent,
and the OS accent on desktop.

## Persistence

Theme and accent preferences are stored in `localStorage` through
[`BrowserStorage`](src/app/services/browser-storage.ts), which keeps them in sync
across tabs.
