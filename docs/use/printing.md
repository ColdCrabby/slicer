# Sending to your printer

Once a slice succeeds, the result button bottom-right offers three things:

- **Download G-code** — save the file and move it yourself (SD card, USB, your
  own upload).
- **Just upload** — copy the file to the printer without starting it.
- **Upload & print** — copy it and start the print, after you confirm.

It remembers which you chose last, so the one you use most becomes the default
click. Upload & print still asks every time, because it moves real hardware. And
once you change anything after slicing, the button steps aside until you
re-slice, so a file that no longer matches the plate is never sent.

Downloading works everywhere and needs no setup. Uploading needs a connected
printer.

## What's supported today

**Klipper via Moonraker** — Mainsail, Fluidd, and anything else that speaks
Moonraker. This is the connection that detects, probes, uploads and starts
prints.

OctoPrint, PrusaLink and Bambu Lab can be **saved** on a printer profile, but
sending to them isn't implemented yet. Their status shows as *unsupported*
rather than pretending to work. Download the G-code and upload it through their
own web interface in the meantime.

## Connecting a printer

**Settings → Printers → Add printer**, then either press **Detect** with the
printer's address, or fill in the connection section by hand:

- **Host** — a hostname (`mainsailos.local`) or an IP (`192.168.1.42`). It can
  include a scheme or port if yours is unusual.
- **Port** — normally left alone.
- **API key** — only if your Moonraker requires one.

**Detect** is the shortcut worth taking: it confirms the machine is reachable
*and* fills in bed volume, nozzle and kinematics from the printer.

## The status dot

Every printer card and the home screen show a live status dot, re-checked
periodically.

| Dot | Meaning | What to do |
| --- | --- | --- |
| **Green** | Online | Nothing |
| **Amber** | Checking, the host answered with an error, or the connection type is **Not supported** yet | Wait, check the API key — or download and upload manually |
| **Grey** | **Offline** (nothing answered), or a local profile with no connection | Check the address and that the printer is on — or nothing, if it's simply switched off |

Offline is grey rather than red on purpose: a printer that's switched off isn't a
fault. Hover a card for the reason behind its status, and click it to open that
printer's settings.

The dot reflects an actual probe, not a saved flag. If it's green, something
answered just now.

## Why the desktop app connects better

In the desktop app (and on a self-hosted server) the probe and the upload happen
in the native process, over the OS's network stack.

In a plain browser tab they can't. Moonraker doesn't send the CORS headers a
browser demands, so a direct request from a web page is usually blocked — even
though the printer is sitting right there and reachable. When that happens
you'll see a distinct *blocked by browser security* status rather than a
misleading "offline".

**If you print from the same machine you slice on, use the desktop app.** If you
want the browser, [self-host](/teams/self-host) — then the server does the
talking and the browser never has to.

## While it uploads

A pill at the top of the scene fills as the upload runs, then turns green or red
in place. Failure carries the reason from the printer — a bad API key and an
unreachable host read differently, on purpose. If you chose to start the print
rather than just send it, the app marks the moment with a brief celebration.

## Filenames

Downloads are named after the plate. On the desktop you get a normal Save-As
dialog; on iPad, the share sheet; in a browser, a normal download.
