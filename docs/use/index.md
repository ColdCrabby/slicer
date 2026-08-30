# Getting started

Cold Crabby turns a 3D model into **G-code** — the file your printer actually
reads. Drop in an STL, pick your printer, press **Slice**, send it off.

This page gets you from nothing to a printable file. Everything else in this
section is detail you can come back for.

## 1. Pick where it runs

You don't have to install anything to try it. Pick whichever fits how you print.

|                      | Best for                                          | What you do                                                    |
| -------------------- | ------------------------------------------------- | -------------------------------------------------------------- |
| **In your browser**  | Trying it out, tablets, locked-down machines      | Open [slicer.maxscopp.de](https://slicer.maxscopp.de/)         |
| **Desktop app**      | Everyday printing on a laptop or workstation      | Install the macOS, Windows or Linux build                       |
| **iPad**             | Slicing on the couch, with touch and pen          | Build it yourself for now — see [Building](/guide/building)     |
| **Your own server**  | A team, a workshop, a print farm                  | See [For teams](/teams/)                                        |

All four behave the same. The slicing engine is literally the same code, so a
model sliced in the browser and the same model sliced on the desktop give you
the same G-code.

::: tip Which should I pick?
Start in the browser. If you like it, install the desktop app — it's quicker on
big models and can reach printers on your network without browser security
getting in the way.
:::

::: details Advanced — what actually differs between them
The browser build compiles the whole pipeline to WebAssembly and never uploads
your model anywhere. The desktop app runs the same pipeline as native code, with
OS file dialogs, native menus, and a direct network path to your printer (no
CORS). A self-hosted server slices server-side and streams progress back over a
WebSocket. Only the _transport_ differs — the geometry does not.
:::

## 2. Open a model

The home screen gives you three ways in:

- **Try the 3DBenchy demo** — a model is already loaded, nothing to find on disk.
- **Open Model to Slice** — pick a file and go straight to the build plate.
- **Empty Workplate** — start with a bare bed and add models yourself.

You can also just **drag a file onto the window** at any time.

**Supported formats:** `.stl`, `.obj`, `.3mf`, up to 500 MB each. A 3MF holding
several parts is split into one object per part, so you can move them
independently.

New models land on the bed and are turned onto their flattest face
automatically. Both behaviours can be switched off — see
[The build plate](/use/plate).

## 3. Check printer and filament

The settings panel on the left has three tabs:

- **Printer** — the machine: bed size, nozzle, firmware.
- **Filament** — the spool: temperatures, cooling.
- **Process** — how to print it: layer height, walls, infill, supports.

Each tab has a dropdown at the top for picking a saved profile. Cold Crabby
ships with sensible defaults, so for a first slice you can usually leave
everything alone.

If your printer isn't set up yet, go to **Settings → Printers → Add printer**.
For a Klipper machine on your network, paste its address and press **Detect** —
bed size, nozzle and firmware fill themselves in. Details in
[Printers, filaments and profiles](/use/profiles).

## 4. Slice

Press **Slice**, bottom right. A progress bar walks through the stages
("Slicing layers", "Generating walls", …) and finishes with something like
`Sliced · 218 layers · 1h 12m`.

The view flips to the **G-code preview** so you can check the result before you
commit plastic to it. Drag the layer slider, or press `↑` and `↓`, to walk up
through the print. See [Reading the preview](/use/preview).

## 5. Send it to the printer

Bottom right again, the result button offers three things:

- **Download G-code** — save the `.gcode` file and copy it across yourself.
- **Just upload** — put the file on the printer without starting it.
- **Upload & print** — send it and start immediately.

Upload and print need a connected printer. Today that means **Klipper via
Moonraker** (Mainsail, Fluidd). Other connection types can be saved but won't
send yet. See [Sending to your printer](/use/printing).

## Where to next

- [The interface](/use/interface) — what every panel and button does
- [The build plate](/use/plate) — moving, rotating, duplicating, arranging
- [Print settings](/use/settings) — what the options mean, in plain language
- [Keyboard shortcuts](/use/shortcuts) — the whole list
- [Troubleshooting](/use/troubleshooting) — when something looks wrong
