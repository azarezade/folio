# Architecture

Folio is a Rust workspace with a Svelte UI in a [Tauri 2](https://tauri.app) shell. All decoding, rendering, syncing and indexing happens in Rust. The UI is a client of one small API.

```
┌──────────────────────────── Folio.app (Tauri) ────────────────────────────┐
│  app/ui  (Svelte 5)                                                       │
│    shell · screens · readers · lib/ui components · tokens.css             │
│         │  call(name, args)      fetch folio://…       listen folio-event │
│  app/src-tauri  ──────────── folio_core::api ─────────────────────────────│
└──────────────────────────────────┬────────────────────────────────────────┘
                                   │
  folio-core     App: index, cache, jobs, device status, transfers; api; export
     ├── folio-library   library layout, index.sqlite (FTS5), render cache, sync engine
     ├── folio-device    Kindle over USB MTP (mtp-rs on nusb), hot-plug, ptpcamerad
     ├── folio-kfx       book formats: detection, DRM, metadata, covers, text for search
     ├── folio-render    strokes → raster (tiny-skia, pencil texture) or vector paths
     ├── folio-pdfout    streaming PDF writer (raster or vector pages)
     └── folio-nmdl      notebook model: pages, strokes, shapes, templates, placements
           └── folio-kdf     KDF container (SQLite + fingerprints), fragments, reachability
                 └── folio-ion   Amazon Ion binary reader/writer, YJ symbol table
  folio-cli      the `folio-cli` command: inspect, convert, sync, push, serve, …
```

## Crates

| Crate | What it does |
|---|---|
| `folio-ion` | Lazy Ion 1.0 binary reader and a small writer, the YJ shared symbol table, and aggregated diagnostics. |
| `folio-kdf` | Opens KDF files: strips the 1 KiB fingerprint records, reads fragments and device edit deltas from SQLite, and runs kfxlib's reachability walk. |
| `folio-nmdl` | The notebook model (a port of kfxlib's notebook code): reading order, pages, strokes (the delta/nibble point codec), brushes, "convert to shape" graphics, templates, and canvas placement for PDF annotations. |
| `folio-render` | Turns strokes into images (stroke outlines with tiny-skia, a seeded pencil texture, highlighter groups) or vector paths (pressure as 16 grey levels). |
| `folio-pdfout` | A streaming PDF writer, so a 1,500-page notebook exports in about 1 s with flat memory. |
| `folio-kfx` | Book formats: tells MOBI/AZW/AZW3/KFX/EPUB/PDF/text apart by their bytes, detects DRM, reads EXTH/OPF metadata and covers, and extracts text for search (PalmDoc, EPUB spine, PDF pages via lopdf). |
| `folio-device` | A blocking facade over `mtp-rs`: list, read, upload, delete and mkdir, hot-plug events, and freeing the device from macOS's `ptpcamerad`. |
| `folio-library` | The on-disk library, `index.sqlite` (items, reading positions, titles, settings, FTS5 text), the LRU render cache, the sync planner and runner, and push and delete. |
| `folio-core` | The application: `App` (index, cache, notebook LRU, background jobs, transfers, device status), the JSON API, the parallel PDF export, and PDF handwriting overlays and export. |
| `folio-cli` | The command line (`folio-cli --help`). It also has `folio-cli serve`, a dev bridge that runs the same API over HTTP and server-sent events. |
| `folio-app` | The Tauri shell: one `call` command, the `folio://` scheme for page images and files, `folio-event` events, and native menus. |

## The API

The UI reaches the core in three ways, identically in the app and in the dev bridge:

- `call(name, args) → JSON` covers commands and queries: `items`, `notebook`, `sync`, `export`, `device_ls`, `search_text`, `book_annotations`, and more (see `crates/core/src/api.rs`).
- `fetch(path)` returns bytes:
  - `notebook/<id>/page/<n>?w=`, a rendered notebook page
  - `thumb/<id>`
  - `file/<id>`
  - `annotation/<id>/page/<n>?w=`, PDF handwriting as an ink-only PNG
- Events: `job` (progress), `job_log`, `device`, `transfer`, `library`.

Long work runs as cancellable background jobs: sync, export, push, pull, free the Kindle, indexing books for search.

## The library on disk

```
~/Library/Application Support/Folio/
  notebooks/<uuid>.nbk, <uuid>.png        handwritten notebooks + device thumbnails
  annotations/<uuid>!!PDOC!!notebook.nbk  handwriting on PDFs
  books/purchased|sideloaded/…            books and documents, with the Kindle's subfolders and .sdr sidecars
  screenshots/
  .sync/                                  the Kindle's sync database (opt-in)
  .folio-sync.json                        sync manifest: what came from where, sizes, change counters
  index.sqlite                            the index (rebuilt from the files if deleted)
~/Library/Caches/Folio/                   rendered pages and thumbnails (safe to delete)
```

## How a sync works

1. Open one MTP session.
2. Walk `.notebooks/`, `documents/` (keeping subfolders, skipping dot-folders) and `screenshots/`.
3. Plan: a file is copied if it's new, if its size changed, or, for SQLite notebooks, if the file change counter changed (a 100-byte partial read). The Kindle reports no dates over MTP.
4. Copy each file to `name.part`, then rename it.
5. Update the manifest. Nothing on the Kindle is changed.
6. Scan the library into `index.sqlite`: metadata, covers, DRM and page counts.
7. Queue a background job that indexes new books' text for search.

Nothing is converted during a sync. Pages are rendered when you view them, and cached. PDFs are made only when you export.

## The UI

`app/ui/src`:

- `lib/styles/tokens.css`: the `--fo-*` design tokens, light and dark. Components use only these.
- `lib/ui`: the components (Button, Segmented, SearchField, Switch, SidebarRow, ItemCard, JobRow, Toast, Banner, ConfirmSheet, …). Open the UI with `?gallery` to see them.
- `shell`: sidebar, toolbar, status bar, activity, sheets, Settings.
- `screens`: Library, My Clippings, Search, Kindle, Files, Transfers.
- `readers`: the paged reader (notebooks, PDFs via PDF.js), the book reader (EPUB, MOBI and AZW3 via the vendored [foliate-js](https://github.com/johnfactotum/foliate-js)), text, and "can't be opened".

Book pages load as `blob:` frames under a strict CSP, so scripts inside books never run.
