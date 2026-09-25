# Library — Every Model, Once

This module keeps an index of every model that has reached a plate, in any
runtime, so the next plate can be built from what the user already has instead
of from a file picker.

> **One object, one entry.** The same model dropped in twice, downloaded again
> under another name, or re-exported by another program is one entry with
> several places it can be read from.

## Why it exists

A workplate names its models by `file_id`, and that id only lives as long as the
plate. Close the plate and the model is something to go and find again — which
on an iPad means the system file picker, every time. The library is the index
that outlives plates, with a thumbnail per entry so it can be browsed as
pictures.

## The contract

- **Matching lives in Rust and compiles everywhere.** `Library::import` decides
  whether a file is new, the same bytes again, or the same shape in different
  bytes. The browser build calls it through wasm (`libraryImport`); no runtime
  has its own copy of the rule.
- **The engine has no renderer.** Thumbnails are drawn by the webview from the
  engine's own parse of the file and handed back as PNG bytes — the same split
  as the G-code thumbnail.
- **The user's files are never modified or deleted.** Removing an entry deletes
  the library's own copy and its thumbnail. A referenced file is left alone.
- **Reaching a plate is what records a model.** Every path onto a plate goes
  through `Slicer.startWorkplate` or `WorkplateObjects.addFile`, and both call
  `ObjectLibrary.remember`. On the server, the upload handler records instead.

## Matching

| Key | What it catches | Cost |
| --- | --- | --- |
| Content hash (SHA-256) | Copies, re-downloads, the same file in two folders | Read the bytes |
| Shape digest | The same object re-exported: ASCII vs binary STL, another exporter | Parse the model |

The shape digest covers part count, triangle count, bounding-box extents,
volume and surface area, rounded to four significant figures. Extents rather
than corners, so a model exported at another origin still matches. A file whose
hash is already known is never parsed, which is what makes a rescan cheap.

## Copies and references

A location is either a **copy** the library owns or a **reference** to a file
the user owns. `StorageMode` picks what an import produces: `copy`,
`reference`, or `both` (a reference plus a copy to fall back on).

| Runtime | Modes offered | Why |
| --- | --- | --- |
| Desktop | copy · reference · both, plus watched folders | It can reopen any path later |
| iPad / iOS | copy only | A picked file arrives as a throwaway copy; nothing else is reachable later |
| Cloud | copy only | The server cannot see the browser's disk |
| Browser | copy only (IndexedDB) | There is no disk |

On iOS the copies live in `Documents/Models`, which the Files app shows as
*On My iPad › Cold Crabby › Models*. A model saved there from Safari or AirDrop
is in the library at the next scan — the iPad's answer to watched folders.

## Shape on disk

```
<config_dir>/library/
├── index.json        the Library document
├── thumbs/<id>.png   one per entry
└── models/           copies (Documents/Models on iOS)
```

A scan walks the models folder and every watched folder, up to four levels
deep, skipping any file whose path, size and mtime are unchanged. An entry whose
only files were library copies, all since deleted, is dropped — on an iPad that
is the user deleting a model in Files. A missing *reference* is kept and marked
missing: an unplugged drive has not deleted anything.

## Non-goals

- **No renderer**, as above.
- **No sync between runtimes.** The desktop, the server and a browser each have
  their own library, like each has its own profile store.
- **No external folders on iPad yet.** Reaching a folder outside the app's
  container needs a security-scoped bookmark, which the dialog plugin cannot
  produce; the Files-visible `Models` folder covers the common case.

## See also

- [mod.rs](mod.rs) — the document and matching · [store.rs](store.rs) — the
  filesystem store · [wasm.rs](wasm.rs) — the browser binding
- [`ui/src/app/services/library/`](../../ui/src/app/services/library/) — the
  webview's view, per-runtime backends, thumbnails
- [../workplate/README.md](../workplate/README.md) — the plate document that
  names files by id
