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
Klippain's `START_PRINT`) and writes the matching G-code. The first screen lists
every value it read, each one beside the section of `printer.cfg` it came from,
so you can check it against your own config without leaving the page.

The printer is ready to add from that first screen — press **Add as-is**.
Anything the configuration *can't* settle is put to you afterwards as one short
question per screen, and none of them are required.

Each question names what it is about, says why we leaned the way we did, and
shows the config section behind it. A machine with two generic fans is asked
about each fan separately, by name, because Klipper cannot say whether a given
one cools prints, filters the air, or vents the electronics bay — and the safe
answer differs per fan. Every answer is an ordinary setting afterwards, so
changing your mind never means running this again.

Telling us a fan blows on the part does more than switch it on. It joins the
part-cooling fan on the layer-time curve and gains the boosts that only make
sense for a second fan: extra airflow over bridges and on very short layers, a
ceiling so the two together cannot overdo it, and a limit on how much the speed
may change from one layer to the next, which is what stops a fan slamming from
off to full and shocking the part.

The full table of a machine's fans — which ones it has, what each is called in
`printer.cfg`, and the speed curve for each — lives under **Fans** in the
printer's own settings, because that is a description of your hardware rather
than of a spool. What the *material* contributes is its cooling ceiling, which
stays on the filament and is one of the settings a single printer can correct
for itself.

::: details When detection can't tell everything
A printer that answers slowly may not return its full configuration. You still
get its build volume and connection — listed without a config section, because
nothing named one — and the first screen says which settings are still carrying
defaults, with a link to fill them in by hand. Machines the
slicer doesn't recognise leave the vendor and model blank for you to name.
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

**Settings → Filaments → Add filament**. Pick a vendor preset or start from
scratch, then give it a name, a colour and a material — that is the whole flow.
Choosing the material sets the nozzle and bed temperatures and how hard the part
fan runs, and everything else lives in the filament's own settings, where it is
grouped and searchable.

The colour is used in the model view if you turn on **Settings → 3D View →
Colour models by filament** — handy when you have several spools and want to see
which is which.

### Fan curves

The **Cooling** group ends with a fan table. Each row is one physical fan and
the curve it follows: a minimum and maximum speed, and the two layer times they
sit between. Layers that print quickly get the maximum, slow ones the minimum,
and anything between is interpolated.

With no rows the filament uses the printer's part-cooling fan at the engine's
defaults, which is what most spools want. Add a row when you need a second fan
driven — a chamber fan for ABS, or an auxiliary one for a big PLA part. On
Klipper you can also give a row the object's own name from `printer.cfg`
(`rscs`, `exhaust_filter`); leave it blank for the default name for that role.

## Adding a print profile

**Settings → Processes → Add profile.** Pick a tuned preset or start from
scratch, then name it and set a layer height. Walls, infill, speeds and supports
are all in the profile's own settings afterwards — the same page that holds
every other process parameter, grouped and searchable, rather than a shorter
copy of it inside the wizard.

Three profiles come built in, and they are a scale rather than three unrelated
recipes — all 0.20 mm, differing in how hard they drive the machine:

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
**Corrections** sits below the machine's own details in every
printer's editor: pick a
material from the dropdown and a card for it appears, holding every setting that
material corrects. Change a value, stop correcting one setting, correct another,
or **Remove all** to drop the material — that one asks twice, since there is no
undo behind it. A new correction opens at the value it is a correction *of*, so
you can see what you are adjusting away from.

In the outline the whole thing is one entry with a material under it, not a
section per material.

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
  **Make default**, **Delete** — or **Restore defaults** on a built-in.

Duplicating and editing beats starting from scratch. Deleting asks first: the
trash button turns red and reads *Delete?*, and a second press deletes.

The top of the editor is the profile itself: its **name**, which you rename by
clicking it and typing, a one-line summary of what it is, and **Set default**,
**Duplicate** and **Delete** beside it. A small *Saved* appears after each
change — edits save as you make them; there is no Save button to forget.

### Built-ins are yours to edit

The printers, filaments and processes that ship with the app are starting
points, not locked templates. Change anything — the name included — and it
saves like any other edit. They cannot be deleted, because every fallback
resolves to one, so the editor offers **Restore defaults** instead: it puts the
shipped values back (your labels stay). Duplicate one first only if you want to
keep the original alongside your version.

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
folded, so the whole editor fits on screen as a dozen lines. Click one to
open it; that only opens it, and never moves the editor — it is the settings
listed underneath that take you somewhere. **Expand all** at the top does the
lot.

A line runs down its left edge through a dot for each section, stepping in
under an open section's settings and back out below them. The bright stretch of
that line is exactly what the editor is showing: it slides as you scroll, grows
when more fits on screen, and the dots it covers fill in. The section you are
scrolled to is named in the accent colour, and when the outline is longer than
the window it scrolls itself to keep the bright stretch in view — until you
scroll the outline yourself.

Above it, **Filter settings** narrows the outline as you type, and `Ctrl`/`Cmd`
+ `F` puts the cursor there from anywhere on the page. Matches stay grouped
under their own sections, which is the part that helps: typing `gap` shows you
that there is one in Walls, one in Infill, one in Support and two in Speed —
*where* each lives, not just that it exists. It is the fastest way to a setting
you can picture but cannot name.

**A search ignores the folding.** Asking where a setting is and being handed a
closed section would be no answer, so every match is listed whatever state its
section was in.

**It appears when there is room for it**, and steps aside when there is not —
the editor always gets enough width to lay a setting out on one line first. On a
window that is only just too narrow — an iPad held sideways, or a laptop with a
wide list column — the Settings section list folds itself to icons to make the
room, and opens again when you leave the editor. Press the button beside the
word *Settings* to choose for yourself; the app remembers and stops deciding.

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

The foot of the Settings sidebar says which of these applies to your session,
and **Settings → General → Your library** says it in full.

::: tip Back them up
**Settings → General → Your library → Export profile library** downloads everything
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
