# Printers, filaments and profiles

A profile is a saved set of settings you can pick from a dropdown instead of
re-typing. There are four kinds, and they live in **Settings**.

| | Where | What it holds |
| --- | --- | --- |
| **Printers** | `/settings/printers` | Bed size, nozzle, firmware, network connection |
| **Filaments** | `/settings/filaments` | Material identity, temperatures, cooling, flow |
| **Print profiles** | `/settings/profiles` | Layer height, walls, infill — everything in the Process tab |
| **Labels** | `/settings/labels` | Tags for organising the three above |

## Adding a printer

**Settings → Printers → Add printer** gives you three routes:

**Detect it.** If it's a Klipper machine on your network, paste its address
(`mainsailos.local`, or an IP) and press **Detect**. This is by far the least
error-prone option, and usually there is nothing left to fill in.

Your printer's own configuration answers most of the setup, so the slicer reads
it rather than asking you: build volume and kinematics, nozzle and filament
diameter, the machine's velocity and acceleration limits, pressure advance, how
hot its hotend and bed are allowed to get, and whether it has firmware
retraction or can cancel individual objects. It also
works out which start and end macros your printer uses (`PRINT_START` or
Klippain's `START_PRINT`) and writes the matching G-code. Open **What we read
from your printer** on the first screen to see every value and which section of
`printer.cfg` it came from.

You can add the printer right there. What the configuration *can't* settle is
put to you as a short question or two, each with an answer already picked and a
line explaining why — for example what an extra fan is for, since Klipper can't
say whether it cools prints or an electronics bay. Skip them with **Just add
it** and change anything later in the printer's settings.

::: details When detection can't tell everything
A printer that answers slowly may not return its full configuration. You still
get its build volume and connection, and the wizard falls back to the manual
steps for the rest. Machines the slicer doesn't recognise leave the vendor and
model blank for you to name.
:::

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
  Detection turns this on by itself when your Klipper config has the
  `[exclude_object]` module.

## Adding a filament

**Settings → Filaments → Add filament**. Name, material, colour, nozzle and bed
temperatures, cooling. The colour is used in the model view if you turn on
**Settings → General → Use filament color for models** — handy when you have
several spools and want to see which is which.

## Adding a print profile

**Settings → Print Profiles → Add profile.** Three come built in, and they are a
scale rather than three unrelated recipes — all 0.20 mm, differing in how hard
they drive the machine:

| Preset | Walls / infill | For |
| --- | --- | --- |
| **Standard** | 120 mm/s | Anything. Slow enough that it cannot embarrass a machine you have not commissioned. |
| **High Speed** | 200 mm/s | A well-built CoreXY with a high-flow hotend. |
| **Maximum** | 300 mm/s | The top of the sensible range — a tuned machine at 30 000 mm/s². |

All three hold the **outer wall and the top surface back**, because those are
what the print is judged by and neither is where the time goes. Going fast on
the inside is what pays for going slowly on the outside.

They fit whatever nozzle they land on. Bead width and first-layer height are
stated as a proportion — `110%` of the nozzle, `120%` of the layer height —
rather than in millimetres, so the same preset lays a 0.44 mm bead on a 0.4 mm
nozzle and a 0.66 mm bead on a 0.6 mm one. Type a number over it and it stays a
number; the `%` button on the field switches between the two.

Two things decide whether the fast ones are honest on your machine:

- **Your printer profile has to carry the speed.** A process asking for
  300 mm/s behind a printer that travels at 150 does not get it. The built-in
  **Generic CoreXY 350 mm** printer is the matching starting point.
- **Flow is the real ceiling**, and it belongs to the spool, not the recipe. Set
  **Max Volumetric Speed** on the filament — around 24 mm³/s for a modern
  high-flow hotend — and every speed above is held to what the hotend can melt.

::: tip Pressure advance stays where you tuned it
No shipped preset sets it. It is calibrated per machine and per spool and lives
in your firmware; a profile that shipped a number would overwrite a calibration
it knows nothing about. Same for retraction on a Klipper machine — the CoreXY
preset turns **firmware retraction** on so the printer's own values win.
:::

## When one printer disagrees with the others

Most settings are true wherever you print. A few are not: how fast a hotend can
melt a material, what pressure advance an extruder needs for it, how much
retraction an elastic filament wants out of that particular drive. These belong
to a **machine and a material together**, so no single number on the filament is
right across three printers.

You do not set these up in advance, and you never make a second copy of a
filament. You correct them where you notice them:

1. On the slice page, change the setting — **Max Volumetric Speed**, say.
2. Press **Sync changes to your profiles**.
3. The row offers two scopes. **This printer only** saves it as that machine's
   correction for the material you have loaded. **Every printer** edits the
   shared filament.

"This printer only" is the default, because it cannot affect anything else you
own. Every other spool of the same material inherits the correction
automatically, so buying more PLA never costs you a new profile.

They are also managed like anything else, in **Settings → Printers**.
**Correct a material** sits at the top of every printer's editor, and each
corrected material gets its own section further down — and its own entry in the
outline — with every corrected setting as a normal control. From there you can
change a value, stop correcting one setting, correct another, or drop the
material entirely. A new correction opens at the value it is a correction *of*,
so you can see what you are adjusting away from.

Afterwards, the setting shows where its value came from:

```
Max volumetric speed   24 mm³/s
⚙ Corrected for Voron 2.4 · PLA
```

That line is there so a number disagreeing with the filament profile you picked
explains itself instead of looking like a fault. It appears only when a
correction is actually in play — switch to PETG, or to another printer, and the
filament's own value comes back.

::: details Which settings can be corrected this way
Temperatures (nozzle and bed, including first layer), maximum volumetric speed,
flow ratio, pressure advance, retraction length and speed, Z hop, and the fan
ceiling.

The list is deliberately short. Anything else is true wherever you print it, so
it belongs in the printer, filament or print profile that owns it — and offering
a per-machine copy of everything would leave you holding two overlapping sets of
settings instead of one.
:::

## What your machine cannot do

A printer profile records three limits read straight off your machine:
**Hotend temperature limit**, **Bed temperature limit** and **Machine
acceleration limit**. None of them is a value anything prints at — nothing is
tuned here, and none of them is written into your G-code.

The two temperature limits exist because asking for heat a machine cannot reach
does not fail. The print simply waits for a temperature that never arrives, and
the printer sits hot until you notice. So if you load ABS on a machine whose bed
stops at 80 °C, the slicer says so before it writes the file.

The acceleration limit exists for the **print-time estimate**. A fast print
profile asking for 25 000 mm/s² on a printer commissioned at 3 000 is fine — the
file carries the higher number, the firmware clamps it, and the part comes out
correctly. What is not fine is an ETA calculated as though the machine reached
it. The estimate is held to whichever is lower, and a note says so.

Detection fills both in from your printer's own configuration. A machine you
entered by hand leaves them at `0`, which means "unknown" and warns about
nothing.

## When a preset asks more than a machine can give

Pick a printer and the filament and print-profile dropdowns say, under any
preset that is a stretch for it, what the problem is:

```
Maximum — 0.20 mm
Asks 300 mm/s; Ender 3 tops out at 150
```

```
Generic ABS
Needs 255 °C; this hotend is rated for 240
```

Nothing is ever disabled. Plenty of printers are configured conservatively and
run happily above it, and a print profile asking for speed is exactly what a
print profile is for — the machine simply does what it can. The note is there so
you find out before the print, not during it.

The temperature ones are worth taking seriously, though: unlike a speed the
machine quietly won't reach, a heat target it cannot reach never arrives at all,
and the print waits on it indefinitely.

## Everyday management

All three lists behave the same:

- **Search** by name.
- **Group by** vendor or connection type.
- **Filter by label**.
- **Star** one as the default — that's what a new plate starts with.
- **Right-click** (or long-press on touch) a card for **Labels**, **Duplicate**, **Edit**,
  **Make default**, **Delete**.

Duplicating and editing beats starting from scratch. Deleting asks first.

Editing a profile reaches every plate that uses it, including plates you sliced
weeks ago — that is the point of a profile. The exception is a setting you
changed on a particular plate, which stays as you left it. See
[Changes belong to the plate](/use/settings#changes-belong-to-the-plate).

## Finding a setting in the editor

These three pages are the one place that shows **every** parameter the slicer
has — nothing folded away, nothing behind an *Advanced* step. That is what they
are for, and it is also what makes them long: a printer's editor runs to sixty
settings, a print profile past two hundred.

The **outline** down the right-hand side is the map. Every section starts
folded, so the whole editor fits on screen as a dozen lines. Open one with the
chevron beside it — that keeps you where you are — or click its name to be taken
there and have it open. **Expand all** at the top does the lot. The section you
are currently scrolled to is highlighted, so you never lose your place.

Above it, **Filter settings** narrows the outline as you type, and `Ctrl`/`Cmd`
+ `F` puts the cursor there from anywhere on the page. Matches stay grouped
under their own sections, which is the part that helps: typing `gap` shows you
that there is one in Walls, one in Infill, one in Support and two in Speed —
*where* each lives, not just that it exists. It is the fastest way to a setting
you can picture but cannot name.

**A search ignores the folding.** Asking where a setting is and being handed a
closed section would be no answer, so every match is listed whatever state its
section was in.

**Fold the section list to see it.** Settings is already three columns wide, so
the outline only appears once you collapse the section list on the far left to
icons — the button beside the word *Settings*. It also needs a window wide
enough for the extra column; below that the list and the editor keep the room.

The list column itself is draggable: pull the edge between the list and the
editor to give long profile names the width they need. It stays where you put
it.

## Labels

Labels are a flat set of tags — `PLA`, `prototype`, `customer-work`, whatever
suits. Create them in **Settings → Labels**, assign them to any profile, then
use the label bar to filter long lists — selecting more than one widens the
list rather than narrowing it, so `PLA` plus `PETG` shows both. A card's right-click menu carries a
**Labels** submenu holding the same picker — coloured, searchable, with a tick
beside each one already assigned and a row to create a label that does not exist
yet. It stays open while you pick, so tagging a shelf of profiles is one pass. On a shared machine with a dozen
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
the built-in library. It asks you to type a confirmation first. Because your
plates remember their settings as *changes to a profile*, and those profiles are
being replaced, this clears the per-plate changes too. The same page can clear
slice history or reset the whole app.
