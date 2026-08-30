# For teams and businesses

Cold Crabby is one engine you can put wherever your work happens: on each
person's machine, on a server everyone shares, or in a script that never touches
a UI at all.

This section is for whoever sets that up. If you just want to slice something,
start at [Getting started](/use/).

## Pick a shape

| | Good for | Trade-off |
| --- | --- | --- |
| **Desktop app on each machine** | Small teams, designers with their own printers | Profiles live per machine unless you export/import |
| **One self-hosted server** | Workshops, print farms, shared printers | You run a server |
| **CLI in a pipeline** | Automated slicing, batch jobs, CI | No UI |
| **Public browser build** | Contractors, one-off collaborators | Their profiles live in their browser |

They aren't exclusive. A common arrangement is a self-hosted server as the
shared source of truth for profiles and history, with the desktop app for
whoever wants offline work.

## Why self-host

**Profiles survive.** In a browser tab, profiles live in that browser — clearing
site data loses them. On a server they're stored server-side and every browser
on the network sees the same library.

**Printers actually connect.** A browser can't talk to most printers directly:
Moonraker doesn't send the CORS headers browsers require. A self-hosted server
talks to printers over the network from the server process, so it just works.

**Files stay inside.** Models are uploaded to your server and nowhere else.
Nothing leaves your network.

**Plate history is shared.** A plate sliced at one workstation reopens at
another.

→ [Self-hosting](/teams/self-host)

## Standardise your settings

Two mechanisms, for two different jobs.

**Profiles** are the user-facing library — printers, filaments, print profiles,
labels. Export the library once from a machine that's set up correctly and
import it everywhere else. Exports strip API keys, so they're safe to commit to
a repo or hand out.

**`slicer.toml`** is the machine-facing config: layered defaults for the CLI and
the server. A project-level `slicer.toml` checked in beside a job's models
pins exactly how that job is sliced.

→ [Configuration](/teams/configuration)

## Automate it

The CLI slices without a UI, takes multiple models per plate, arranges them, and
emits machine-readable JSON. That's enough to slice on commit, batch a folder of
parts overnight, or quote a job from its G-code.

→ [Automation and the CLI](/teams/automation)

## Before you commit

**Licensing.** All rights reserved until a licence is chosen. Ask before
deploying commercially.

**Maturity.** Pre-1.0. The engine is tested continuously against reference
models and G-code quality gates, but treat it as software you validate against
your own parts before it prints unattended.

**No accounts.** A self-hosted instance is single-tenant with no
authentication — one shared library, one shared history. Put it behind your own
access control if it's exposed beyond a trusted network.

→ [Data, privacy and licensing](/teams/data)
