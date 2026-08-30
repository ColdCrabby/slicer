# Troubleshooting

## The model

**"Outside the build area"** — part of the model is beyond the print volume.
Use **Centre on bed**, or scale it down, or check your printer profile's bed
size is right. The check uses your bed's actual shape, so a round bed isn't
treated as a square.

**"Overlaps another object"** — two parts intersect. Press `A` to re-arrange, or
move one by hand. Parts that merely touch edge-to-edge don't trigger this.

**The model floats above or sinks into the bed** — turn **Gravity** (`G`) on,
or use **Drop to floor** in the move tool.

**A 3MF came in as several objects** — that's correct. A 3MF is a scene, and
each build item becomes its own object so you can place them independently.

**The file won't open** — Cold Crabby reads `.stl`, `.obj` and `.3mf` up to
500 MB. Anything else (STEP, F3D, blend) needs exporting to one of those first.

## Slicing

**The slice failed** — the status line carries the reason. Most often it's a
model with holes or flipped normals. Repair it in your CAD tool, or run it
through a mesh repair service, and try again.

**It's taking a very long time** — big models with fine layers and dense infill
are genuinely slow. In a browser tab it's slower still, because the whole engine
is running inside the page. The desktop app is markedly quicker on heavy models.

**Nothing seems to change when I re-slice** — identical input produces identical
output, and Cold Crabby will reuse a cached result rather than repeat the work.
Camera movement and the thumbnail don't count as changes. If you changed a
setting, the result *will* differ.

## The preview

**Sparse or missing top surfaces** — increase top layers (Process → Surfaces),
or infill density so the top has something to build on.

**Supports where I don't want them** — raise the overhang threshold angle, or
switch support type. Check the preview again afterwards.

**Stringing between parts** — enable travel moves in the preview to see the
paths, then look at Retraction on the printer profile.

## Printers

**The status dot is red** — nothing answered. Check the address, that the
printer is powered on, and that you're on the same network.

**The status dot says blocked by browser security** — you're in a browser tab
and the printer doesn't send CORS headers. This is a browser restriction, not a
fault. Use the [desktop app](/use/), [self-host](/teams/self-host), or download
the G-code and upload it through your printer's own web interface.

**"Unsupported"** — that connection type isn't implemented yet. Only Klipper via
Moonraker can upload today.

**The upload was rejected** — usually a wrong or missing API key. The
notification carries the printer's own message.

## Settings and profiles

**My profiles disappeared** — if you were running in a browser tab, they were
stored in that browser. Clearing site data removes them. Export a backup from
**Settings → General → Backup & Export**, and consider the desktop app or a
self-hosted server, where they live outside the browser.

**A setting I read about isn't there** — options hide when they don't apply.
`Thin walls`, for instance, only appears with the classic wall generator, since
the Arachne generator handles thin features by construction. Search with
`⌘/Ctrl + F`; if it still doesn't appear, something it depends on is switched
off.

**I want to start clean** — **Settings → Danger Zone** can clear slice history,
reset profiles to defaults, or reset the whole app. Each asks for confirmation.

## Performance

**The 3D view is choppy** — in **Settings → General**, lower **Render
resolution** and **Preview detail**, and turn **Anti-aliasing** off.

**A big G-code preview is slow to scrub** — switch from *all layers* to *current
layer only*.

## Still stuck

Check what version you're on in **Settings → General**, then open an issue on
[GitHub](https://github.com/max-scopp/slicer-engine/issues) with the model, your
settings, and what you expected.
