# Data, privacy and licensing

## Where your models go

It depends entirely on how you run it, and the difference is real.

| Running as | Your model | Your profiles |
| --- | --- | --- |
| **Browser build** | Never leaves the machine — the whole engine runs in the page | In that browser only |
| **Desktop app** | Never leaves the machine | On that machine |
| **Self-hosted** | Uploaded to your server | On your server |
| **Public instance** | Uploaded to that instance | In your browser |

There is no telemetry, no analytics, and no account system. Nothing phones home.

::: tip For anything confidential
Use the desktop app or self-host. Both keep models inside your own
infrastructure end to end.
:::

## What a self-hosted server stores

| | Where | Contains |
| --- | --- | --- |
| Uploaded models | Work directory | The mesh files people slice |
| Generated G-code | Work directory | Output, plus a cache keyed by scene + settings |
| Slice history | `slicer.db` in the work directory | Past slices, plate metadata, thumbnails |
| Profile library | `profiles.toml` in the config directory | Printers, filaments, processes, labels — **including printer API keys** |
| Configuration | `slicer.toml` in the config directory | Defaults |

To wipe an instance, stop it and delete those two directories.

**Printer API keys are stored in plain text** in `profiles.toml`. Treat that
file as a secret: restrict its permissions, and don't put it in a repository.

::: warning There is no authentication
A self-hosted instance is single-tenant. Anyone who can reach the port has full
access — the shared library, all history, and any configured printer. Keep it on
a trusted network or front it with your own access control.
:::

## Exports are safe to share

The profile library export strips every credential field — API keys, tokens —
from both the bundle and the single-file shape. That's deliberate: an export
exists to be handed over, mailed, or committed. Whoever imports it re-enters
their own keys.

## Retention

Nothing expires on its own. Slice history and cached G-code accumulate until you
clear them. **Settings → Danger Zone → Clear slice history** does it from the
interface; deleting the work directory does it from the outside.

If the work directory is left at its default (system temp), your OS may clear it
for you at unpredictable moments. Set `work_dir` explicitly for anything you
want to keep — see [Self-hosting](/teams/self-host).

## Network access

The engine makes outbound connections in exactly one case: talking to a printer
you configured. Status probes and uploads go from the desktop app or the server
directly to that printer's address. Nowhere else.

The browser build makes no outbound connections at all — it has no native
transport, which is also why printer uploads don't work from it.

## Licensing

**All rights reserved.** No use, reproduction, modification or distribution is
permitted without written authorisation. A licence is yet to be decided.

If you want to deploy this commercially, ask first —
[open a discussion](https://github.com/max-scopp/slicer-engine/discussions).

## Maturity

Pre-1.0. Every change runs against a continuous test suite plus G-code quality
gates that measure real sliced output — wall overlap, unfilled gaps, bead widths
— against reference models, so regressions in print quality are caught
mechanically rather than noticed on a printer.

That said: validate against your own parts before you let it print unattended,
and keep an eye on the preview. The changelog is embedded in every build under
**Settings → What's New**, so you can always see what changed in the version
you're running.
