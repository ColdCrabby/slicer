# Printers, filaments and profiles

A profile is a saved set of settings you can pick from a dropdown instead of
re-typing. There are four kinds, and they live in **Settings**.

| | Where | What it holds |
| --- | --- | --- |
| **Printers** | `/settings/printers` | Bed size, nozzle, firmware, network connection |
| **Filaments** | `/settings/filaments` | Temperatures, cooling, colour |
| **Print profiles** | `/settings/profiles` | Layer height, walls, infill — everything in the Process tab |
| **Labels** | `/settings/labels` | Tags for organising the three above |

## Adding a printer

**Settings → Printers → Add printer** gives you three routes:

**Detect it.** If it's a Klipper machine on your network, paste its address
(`mainsailos.local`, or an IP) and press **Detect**. Bed size, nozzle diameter,
firmware flavour and kinematics come back from the printer itself, and the
connection is configured in the same step. This is by far the least error-prone
option.

**Pick it from the catalog.** A library of common machines, pre-filled.

**Enter it manually.** Bed shape and size, nozzle, kinematics, firmware
flavour, origin offset, and optionally a connection.

Some settings are on the printer rather than a print profile because they
describe the *machine*, not the print:

- **Preferred print angle** — many CoreXY machines lay parts down best at 45°.
  Auto-arrange uses it.
- **Gantry clearance height and radius** — how much room the printhead needs.
  Used to warn you about sequential printing.
- **Can cancel individual objects** — whether the firmware supports it.

## Adding a filament

**Settings → Filaments → Add filament**. Name, material, colour, nozzle and bed
temperatures, cooling. The colour is used in the model view if you turn on
**Settings → General → Use filament color for models** — handy when you have
several spools and want to see which is which.

## Adding a print profile

**Settings → Profiles → Add print profile**. A print profile is nothing more
than a saved set of Process settings, so the quickest way to make one is to get
a plate slicing the way you like, then save those settings as a profile.

## Everyday management

All three lists behave the same:

- **Search** by name.
- **Group by** vendor or connection type.
- **Filter by label**.
- **Star** one as the default — that's what a new plate starts with.
- **Right-click** (or long-press on touch) a card for **Duplicate**, **Edit**,
  **Make default**, **Delete**.

Duplicating and editing beats starting from scratch. Deleting asks first.

## Labels

Labels are a flat set of tags — `PLA`, `prototype`, `customer-work`, whatever
suits. Create them in **Settings → Labels**, assign them to any profile, then
use the label bar to filter long lists. On a shared machine with a dozen
printers this is the difference between a list and a mess.

## Where profiles are stored

This depends on how you run Cold Crabby, and it matters.

| Running as | Stored | If you clear your browser |
| --- | --- | --- |
| **Browser** | In the browser only | **They're gone** |
| **Desktop app** | On the machine, next to the engine | Safe |
| **Self-hosted** | On the server, synced to every browser | Safe |

The Settings sidebar tells you which of these applies to your session, in as
many words.

::: tip Back them up
**Settings → General → Backup & Export → Profile library** downloads everything
— printers, filaments, print profiles, labels — as TOML. Two shapes:

- a **bundle**, one file per profile plus a manifest and README
- one **`profiles.toml`**, the same format the command line reads

Printer API keys are stripped from both, so the file is safe to hand to a
colleague or commit to a repo.
:::

::: details Advanced — the on-disk library
The desktop app and a self-hosted server keep `profiles.toml` next to
`slicer.toml` in the platform config directory. A category (printers, filaments,
processes, labels) is written whole on every change, last writer wins. In a
self-hosted setup a change made in one browser tab nudges the others to refetch,
so two people editing don't see stale data. Concatenating a bundle's files in
name order reproduces a valid `profiles.toml`, order intact.
:::

## Starting over

**Settings → Danger Zone** has **Reset profiles to defaults**, which restores
the built-in library. It asks you to type a confirmation first. The same page
can clear slice history or reset the whole app.
