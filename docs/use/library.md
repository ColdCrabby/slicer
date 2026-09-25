# The library

Every model you put on a plate is kept in the **Library**, once, with a picture
of it. Open **Library** in the side rail to build a plate from models you
already have instead of hunting for the files again.

## Using a model from the library

- **Click** a model to see it — drag the picture on the right to turn it.
- **Put on a new plate** starts a plate with it.
- **Add to plate** puts it on the plate you have open. Double-click (or press
  `Enter` on) a model to do the same.
- **Search** by name, and sort by **Recent**, **Added** or **Name**.
- Click the name above the details to rename it. The file keeps its own name.

## Adding models

Models join the library by themselves when they reach a plate — however they got
there. To add some without opening a plate, use **Add models** or drop files on
the library page.

The library never keeps the same model twice. Dropping in a file it already has,
a renamed copy, or the same model saved again by another program just points the
existing entry at it.

## Where models are kept

Set this in **Settings → Library**.

| Setting | What it does |
| --- | --- |
| **Copy** (default) | Copies each model into the library, so it stays even if you move or delete the original. |
| **Link** | Leaves models where they are and remembers where. Nothing is copied, but a moved or deleted file shows as missing. |
| **Both** | Remembers where each model is *and* keeps a copy to fall back on. |

The choice is on the desktop app only. The iPad and iPhone app, the browser
version and a shared slicer server always copy.

### Folders (desktop)

Add a folder under **Settings → Library → Folders** — your downloads, say — and
every model in it, and in folders inside it, appears in the library on its own.
Press the refresh button on the library page to look again.

### iPad and iPhone

The library is a folder in the **Files** app: **On My iPad › Cold Crabby ›
Models**. Save a model there from Safari, Mail or AirDrop and it shows up in the
library, with no file picker. Deleting it there removes it from the library.

::: details Advanced — how duplicates are found
Two files are the same model if their bytes match, or if they describe the same
shape: same number of parts and triangles, same size, same volume and surface
area. That catches an STL saved as ASCII instead of binary, or exported again by
another program. A model moved to a different position in its file still
matches; a scaled or rotated one does not.
:::

::: details Advanced — removing a model
**Remove from library** deletes the library's copy and its picture. A linked
file in one of your folders is never deleted — the library only forgets it.
:::
