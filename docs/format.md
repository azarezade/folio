# Kindle Scribe Notebook Format: Implementation Notes

These notes cover how to decode a Kindle Scribe notebook (`nbk`) and the other
files Folio reads from the device. They are distilled from John Howell's kfxlib
(GPLv3, part of the [KFX Input calibre plugin](https://www.mobileread.com/forums/showthread.php?t=291290)),
which Folio's decoders port to Rust, and from what Folio observed on a real
Kindle Scribe. File and line references (`yj_structure.py:718`, …) point into kfxlib.

kfxlib is the authority on notebook decoding. Where these notes and the Rust
code in `crates/` disagree, check both against kfxlib.

Notation:
- `$NNN` is a YJ shared-table symbol ID, as kfxlib prints it.
- `nmdl.*` names are local symbols defined by the notebook itself.

---

## 1. Container: KDF (an SQLite database)

### 1.1 Fingerprint removal

Source: `kpf_container.py:401` (`SQLiteFingerprintWrapper`). Do this before opening
the file with SQLite.

1. Check whether `data[1024:1028] == FA 50 0A 5F`. If not, the file is plain SQLite
   and needs no changes.
2. Otherwise, set `offset = 1024` and loop while `len(data) >= offset + 1024`:
   1. Check that the 4-byte signature at `offset` is `FA 50 0A 5F`.
      - If it is not, stop and use the original data unchanged (log an error).
   2. Remove bytes `[offset, offset+1024)`.
   3. Add `1024 * 1024` to `offset`. This is measured in the data *after* the removal.
3. The log line "Removed N KDF SQLite file fingerprint(s)" reports N.
   - Example: 84 fingerprints for an 84 MB file.

### 1.2 Tables

Source: `kpf_container.py:40`. Compare schemas by exact SQL text.

| Table | Columns | Use |
|---|---|---|
| `fragments` | `id, payload_type, payload_value` | Main fragment store |
| `local_delta_fragments` | `id, payload_type, payload_value, deleted` | Device edits; they **override** `fragments` rows with the same `id`. Rows with `deleted != 0` hide that id. |
| `local_action_fragments` | `action_id, payload_type, payload_value` | Edit/action log. It marks the file as a notebook. kfxlib only logs it. |
| `kfxid_translation` | `eid INTEGER, kfxid` | Maps integer EIDs inside values to symbolic ids (`deref_kfx_ids`, `kpf_container.py:355`) |
| `fragment_properties` | `id, key, value` | `key == "element_type"` gives a fragment's type when its payload is not Ion |
| `capabilities` | `key, version` | Becomes fragment `$593` |
| `gc_fragment_properties`, `gc_reachable`, `index_info`, `index_*` | | Ignore. `index_*` tables appear only in dictionaries. |

**Load order** (it matters for the symbol table):

1. The `$ion_symbol_table` fragment.
2. The `max_id` fragment.
3. All other fragments.

At each step, read delta rows first and then the `fragments` rows not overridden by
a delta.

### 1.3 Payloads

- **`payload_type == "blob"`**:
  - If it starts with `EA 44 52 4D 49 4F 4E EE` (`\xeaDRMION\xee`), it is DRM.
    Notebooks never are.
  - If it starts with the Ion version marker `E0 01 00 EA`, it is an Ion value.
    The value is **annotated**, and the first annotation is the fragment type
    (e.g. `$260`, `$259`, `$608`). A second annotation `$608` may appear and is
    allowed.
  - Otherwise it is a raw resource blob (`$417`).
  - A payload that is exactly the 4-byte version marker is an empty fragment.
- **`payload_type == "path"`**: an external resource path. These appear in KPF
  books, not in notebooks.

### 1.4 Symbol table

- **`$ion_symbol_table` fragment**: an Ion symbol table that imports the shared
  table `YJ_symbols` and defines local symbols (e.g. `nmdl.*`).
- **`max_id` fragment**: an integer. Treat it as importing `YJ_symbols` with
  `max_id - 9`.
  - 9 is the size of the Ion system symbol table.
  - If `max_id` is larger than the table, pad with unknown symbols; if smaller,
    truncate.
- The `YJ_symbols` table itself is in kfxlib's `yj_symbol_catalog.py`. Export it
  to a data file.
- New firmware can reference IDs beyond the table, so unknown symbols must not be
  fatal.

## 2. Ion binary

Port kfxlib's `ion_binary.py` (623 lines). It covers the standard Ion 1.0 binary
types:
- null, bool, int, float, decimal, timestamp
- symbol, string, clob, blob
- list, sexp, struct
- annotation wrappers

KDF values use only a subset, but implement all of them.

## 3. Notebook structure

Notebook decoding starts in `yj_to_epub_content.py:115` (`process_section`).

- **Sections**:
  - The section name is `$174`.
  - Page templates are `$141`, a list. Notebooks use `[0]`.
- **Page section**: the section has `nmdl.canvas_width`.
  - Handled at `yj_to_epub_notebook.py:63`.
  - Fields:
    - `nmdl.canvas_width`, `nmdl.canvas_height`
    - `nmdl.normalized_ppi`: expected to be 2520
    - `nmdl.template_id`: optional; defaults to the notebook default
    - `nmdl.inline_placement_type`: `$670`/`$669`
  - Known canvas sizes:
    - 15624×20832 (portrait; 6.2×8.27 in at 2520 ppi)
    - 13726×7350
    - widths 3906, 13734, 3066, 6132, 12264
    - heights over 15000
  - Anything else logs a warning but still works.
- **Template section**: the section has `nmdl.template_type`.
  - Handled at `yj_to_epub_notebook.py:143`.
  - Template type names look like `dotted_1860x2480` or
    `narrow_rule_margin_1860x2480`.
  - The template body is generic YJ content that kfxlib renders to SVG in a
    1860×2480 coordinate space.
  - It is stretched over the page canvas, drawn **under** the strokes.
- **Template list**: reading order `[1]` with `$178 == "note_template_collection"`
  lists the template ids. Page order comes from the main reading order. Verify the
  exact symbols with the Python reference harness.

### 3.1 Content tree walk

Source: `process_notebook_content`, `yj_to_epub_notebook.py:180`.

- A **symbol** refers to a `$608` fragment. Resolve it and recurse.
- A **struct** with `$159` (content type) == `$270` is a container:
  - `$156` is the layout. It must be absent or `$323`.
  - `$146` is a list of children. Recurse into each.
  - Or `$176` names a story fragment (`$259`) whose `$146` children you recurse
    into.
  - If there is no layout and the struct has `nmdl.type`, it is a stroke node.
- **`nmdl.type`**:
  - `nmdl.stroke_group`: an SVG `<g>` wrapper. It also has `nmdl.chunked=true` and
    `nmdl.chunk_threshold=50`.
  - `nmdl.stroke`: a stroke (§4).

## 4. Strokes

Source: `scribe_notebook_stroke`, `yj_to_epub_notebook.py:230`.

### 4.1 Fields

| Field | Meaning |
|---|---|
| `nmdl.brush_type` | 0 original pen, 1 highlighter (opacity 0.2), 5 pencil, 6 fountain pen, 7 marker (if thickness varies) or pen. Unknown values: log and draw as a pen. |
| `nmdl.color` | Integer RGB; 0 = black. Non-zero values are logged. |
| `nmdl.random_seed` | Seed for the pencil-texture random generator |
| `nmdl.stroke_bounds` | `[x0, y0, x1, y1]` in canvas units |
| `nmdl.thickness` | Float, in canvas units. Named sizes per brush are in `THICKNESS_CHOICES` (`yj_to_epub_notebook.py:40`). |
| `nmdl.stroke_points` | A struct (§4.2) |

### 4.2 Point arrays

`nmdl.stroke_points` holds:
- `nmdl.num_points`
- per-point arrays: `nmdl.position_x`, `nmdl.position_y`,
  `nmdl.density_adjust_factor` (DAF), `nmdl.thickness_adjust_factor` (TAF),
  `nmdl.tilt_x`, `nmdl.tilt_y`, `nmdl.pressure`

Each array is a blob in this delta encoding (`decode_stroke_values`,
`yj_to_epub_notebook.py:~524`):

```
bytes 0-1   01 01                     signature
bytes 2-5   u32 LE                    count (must equal num_points)
then        ceil(count/2) bytes       4-bit instructions, high nibble first;
                                      if count is odd the final low nibble is padding (must be 0)
then        operand bytes, consumed in order

for each instruction:
  n = instr & 3
  if instr & 4: increment = n                   (immediate small value)
  else: increment = 0 if n==0
                    u8        if n==1
                    u16 LE    if n==2
                    u8 + (u16 LE << 8)  if n==3 (24-bit; flagged as unexpected)
  if instr & 8: increment = -increment          (negative zero is an error)
  i == 0:  change = 0; value = increment
  i > 0:   change += increment; value += change   (second-order delta)
leftover bytes after the last value = error (log)
```

### 4.3 Building points

Positions are relative to the stroke bounds.

```
x = position_x[i] + bounds[0];  y = position_y[i] + bounds[1]
variable_density   = any(DAF != 100)
variable_thickness = any(TAF != 100)
taf = TAF[i] if variable_thickness else 100;  taf = (taf // 10) * 10     (QUANTIZE_THICKNESS)
daf = DAF[i] if variable_density  else 100
t = round(thickness * taf / 100);   d = daf / 100
skip a point whose (x, y) equals the previous point's
valid ranges: TAF 0..1000, DAF 0..300 (log if outside)
```

### 4.4 Rendering a stroke with constant density (vector)

- It becomes an SVG group:
  - `stroke = color`, `stroke-width = round(thickness)`, `fill = none`
  - round line caps and joins
- Points are split into paths wherever `t` or `d` changes.
  - Each new path starts from the 2 previous points (`INCLUDE_PRIOR_LINE_SEGMENT`),
    so pieces overlap and leave no gap.
- Each path is `M x y L x y …`. Set a path's `stroke-width = round(t)` when it
  differs from the group default.
- Highlighter strokes go inside a shared `<g opacity="0.20">` at the root of the
  page SVG.

### 4.5 Rendering a stroke with variable density (pencil texture, raster)

This is the fixed algorithm. The original had bugs: dotted lines and a
divide-by-zero crash. Source: `yj_to_epub_notebook.py:~392-470`.

```
S = PNG_SCALE_FACTOR = 8         (bitmap pixel = 8 canvas units; new app: make configurable, default 4)
GAMMA = 3.5;  FEATHER = 0.75
per point: x0 = (x - bounds0) // S;  y0 = (y - bounds1) // S;  r0 = t / (2*S);  keep d

interpolate between consecutive points (recursive midpoint subdivision):
  if dist((x1,y1),(x2,y2)) > max(r1, r2, 1.5):
      x3 = (x1+x2)//2;  y3 = (y1+y2)//2          # integer pixel midpoint
      r3 = (r1+r2)/2;   d3 = (d1+d2)/2           # FLOAT — originally //, which zeroed them
      recurse(p1, p3); recurse(p3, p2); append p3
  # threshold 1.5 (not 1): with integer midpoints a diagonal neighbour pair recurses forever

bitmap W×H = bound_width//S × bound_height//S;  density map zeros
prng = Random(seed = nmdl.random_seed)           # Python's Mersenne Twister; exact parity optional
for each point (x, y, r, d):
  if r <= 0: r = 0.5                             # single pixel; avoids divide by zero
  D = 1 - (1 - clamp(d, 0, 1)) ** GAMMA
  R = ceil(r * 1.5)
  for each pixel (px, py) in the square of radius R around (x, y), inside the bitmap:
     rel = dist((x,y),(px,py)) / r
     if rel <= FEATHER:                           density[p] = max(density[p], D)
     elif rel <= prng.random() * (2 - FEATHER):  density[p] = max(density[p], D * ((2 - FEATHER) - rel))
pixel is ink iff prng.random() < density[p]      (1-bit; or palette for non-black colours)
the bitmap is placed at (bounds0, bounds1) with size bound_width × bound_height
```

### 4.6 Rendering a stroke with variable density (vector mode)

This is Folio's default vector export style.

- Use the §4.4 path-splitting, with the density mapped to a grey level:
  ```
  D = round((1 - (1 - clamp(d,0,1)) ** 3.5) * 16) / 16
  grey = adjust_color_for_density(color, D)
  ```
  `adjust_color_for_density` takes the luminance `lum` of `color` and returns
  `255 - int((255 - lum) * D)` as grey.
- Use solid grey, not translucent black. Translucent black made dark beads where
  path pieces overlap.

## 5. Harmless warnings

These appear constantly on real files. Aggregate them and show them at debug level.

- `content type $270 has extra data: {nmdl.origin_stroke_id: …}`
  - It probably links a copied or moved stroke to its source. Thousands appear per
    notebook.
- `Unknown com.amazon.kindle.nmdl feature: note-5`. Newer firmware; known features
  include `note-2`.
- `Notebook contains unreferenced pages: …`. These are deleted pages still present
  in the database. Do not render them.
- `Undefined EIDs: … (N total)`.
- `Action: action_id=…` dumps from `local_action_fragments`.

## 6. Device layout (Kindle Scribe over MTP)

| Device path | Content |
|---|---|
| `.notebooks/<UUID>/nbk` | Notebook (KDF). The UUID is the notebook id. |
| `.notebooks/<UUID>!!PDOC!!notebook/nbk` | Handwritten annotations on a sideloaded PDF |
| `.notebooks/<UUID>!!EBOK!!notebook/nbk` | Annotations on a purchased book |
| `…/thumbnails/<UUID>.png` | Notebook cover thumbnail |
| `.sync/KSDKNoteSyncDB.sqlite` | Whispersync queue for notebook edits, **910 MB** and rewritten on every edit, so sync copies it only with `--sync-db`. **No titles or folders** (checked 2026-10-24, §8). |
| `documents/` (incl. subfolders, `*.sdr/`) | Books and sideloaded documents; `.sdr` sidecars |
| storage root `*.png/jpg` | Screenshots (`Screenshot_YYYY-MM-DD-HH-MM-SS…`) |

USB: vendor `0x1949`. The interface class is Still Image (0x06) or vendor (0xFF).
The Kindle Scribe tested is product `0x9981` ("Kindle Scribe", USB 2.0, ~15 MB/s
sustained reads). It is not in `mtp_sync.py`'s product list; that code
accepted any Amazon device.

Observed over MTP (2026-10-23, `mtp-rs`):
- **No dates**: ObjectInfo carries no creation or modification dates, in
  listings or `GetObjectInfo`. Detect changes by size and, for SQLite files,
  the header's file change counter (bytes 24–27, big-endian), read with a
  partial read (`GetPartialObject` is supported).
- Storage root: `.notebooks/ .sync/ audible/ documents/ fonts/ screenshots/
  system/ voice/` and a `FILE_SYSTEM_ACCESSIBILITY_FLAG` file. Screenshots go
  in `screenshots/`.
- `.notebooks/<uuid>/` holds `nbk`, `nbk-journal` (empty when idle) and
  `actions.log`. `.notebooks/thumbnails/<uuid>.png` has one PNG per notebook.
- `documents/` contains `.cache/` (skip it), book files, subfolders, and
  `<name>.sdr/` sidecars (`.mbp1`, `.mbs`, `.phl`).

PTP containers use a 12-byte header `<u32 len, u16 type, u16 code, u32 txid>`; the
types are 1 command, 2 data, 3 response.

Operations used:

| Operation | Code |
|---|---|
| GetDeviceInfo | 0x1001 |
| OpenSession | 0x1002 (0x201E means already open; that is OK) |
| CloseSession | 0x1003 |
| GetStorageIDs | 0x1004 |
| GetObjectHandles | 0x1007 |
| GetObjectInfo | 0x1008 |
| GetObject | 0x1009 |
| DeleteObject | 0x100B |
| SendObjectInfo | 0x100C |
| SendObject | 0x100D |

Folders have format 0x3001; new files use format 0x3000. Folio talks MTP through the `mtp-rs` crate (`crates/device`).

## 7. Findings from the Phase 0 harness (2026-10-23)

Observed on 13 real notebooks with `tools/reference/dump_notebook.py`.

- **Fragment references** inside values are `$598`-annotated strings
  (`$598::"lyff…"`), not symbols. A section's `$141[0]` points at its story with
  `$176: $598::"l…"`.
- **Reading orders are stored twice**: in `document_data` (`$538`) and in
  `metadata` (`$258`), both under `$169`. kfxlib errors if they differ.
- **Stroke stories point back at their page section**, so the fragment graph has
  cycles through sections. Walk forward from a section; don't follow edges into
  other sections.
- **Templates can be defined but unused.** `060e7f54`, `4bd89da0` and `f15ed50f`
  define `dotted_1860x2480` (and `wide_rule_margin_1860x2480`), yet no page has a
  `nmdl.template_id` naming them and the notebook default is unset, so kfxlib
  draws no template. The device thumbnail of `060e7f54` has no dots either, so this
  is right. Template ids seen on pages: absent (most) and `$349` (none).
- **Vector graphics (`$272`)** are the Scribe's "convert to shape" results. They
  sit in stroke stories next to the strokes: 8 in `060e7f54`, 11 in `19567df8`,
  128 in `7f2cdb98` and 506 in `88af370a`. kfxlib's notebook walker accepts only
  `$270`, logs "unknown content type" and skips them, so **earlier kfxlib-based tools never
  drew them** and the reference renders lack them. Decoded in
  `crates/nmdl/src/model.rs` (`Graphic`, `Shape`):

  | Field | Meaning |
  |---|---|
  | `$59`, `$58` | **Left** and **top** of the graphic's box on the page canvas (`$58` is top, as in kfxlib's property table; read the other way round until 2026-10-24, which put shapes at transposed positions) |
  | `$56`, `$57` (also `$66`, `$67`) | Box width and height; a number or `{$307: value, $306: unit}` |
  | `$98`, `$549` | Optional affine transform of the whole graphic, and its origin `{$59 left, $58 top}` (rotated shapes) |
  | `$183` | Always `$324` |
  | `$250` | List of shapes |

  Each shape has `$159` kind, colours as signed 32-bit ARGB (`-16777216` = opaque
  black): `$75` stroke (and `$498`, the same value), `$70` fill, `$72` fill
  opacity, `$76` stroke width, optional `$98` transform. Geometry by kind:

  | Kind | Geometry |
  |---|---|
  | `line`, `$273` | `$249` path: opcodes 0 M x y, 1 L x y, 2 Q, 3 C, 4 Z (kfxlib `process_path`), as floats |
  | `ellipse` | `shape_dimensions {cx, cy, radius_x, radius_y}` |
  | `rectangle` | `shape_dimensions {x, y, $56 width, $57 height}` |
  | `polyline`, `polygon` | `shape_dimensions {vertex_list: [x0, y0, …]}`, with a rotation `$98`; arrow heads next to a `line` |

  Seen on pages: line 581, ellipse 49, rectangle 14, polyline 11, polygon 13.
  The template sections draw their dots the same way: `$273` paths filled
  `0xFF888888`.
- **Canvas sizes**: every page in these notebooks is 15624×20832.
- **Brushes seen**: pencil (most), pen, highlighter, marker. No fountain pen and
  no non-zero colours.
- **Stored vs reachable fragments.** kfxlib's fragment counts are taken after
  `check_fragment_usage` (`yj_structure.py:718`), which keeps only fragments
  reachable from the root types; notebooks drop a lot here (e.g. `7f2cdb98`:
  12,777 stored, 3,917 reachable). The reference tables are in
  `crates/kdf/src/sym.rs`. Notebooks count as "KPF prepub" (no ASIN in
  `$490`), which makes `$610` a root and silently drops unreachable
  `$259`/`$260`/`$608`/`$266`/`$391` fragments.
- **Annotations are dropped** before kfxlib's notebook code runs
  (`replace_ion_data` in `yj_to_epub.py:268`), after `$598::` kfxids have been
  turned into names. So page templates (`$608::{…}`) are read as plain structs.
- **Stroke transforms** (`$98` on a stroke, 886 strokes in `55be9342`) are
  `[a, b, c, d, e, f]` affine matrices of 32-bit floats: moved or scaled strokes.
- **Deleted delta rows** (`local_delta_fragments.deleted = 1`) have a NULL
  `payload_type`.
- **Synthetic fragments**: kfxlib adds a `$270` container-info fragment and a
  `$593` list built from the `capabilities` table; both count in its totals.
- Grouped kfxlib messages per notebook are in each `summary.json`
  (`kfxlib_log`). The bulk is `content type $270 has extra data:
  {nmdl.origin_stroke_id: …}`, plus one `Unknown … feature: note-5` and
  "unreferenced pages" warnings.

## 8. Notebook titles: not reachable over USB (2026-10-24)

Looked for the names and folders shown in the Kindle's notebook list. Nothing reachable over MTP has them.

**`.sync/KSDKNoteSyncDB.sqlite`** (910 MB). Its tables:

| Table | Rows |
|---|---|
| `notebook_state` | 18 |
| `notebook_content` | 37 |
| `note_actions` | 120,311 |
| `deltasync_tokens`, `sync_queue`, `notebook_client_address`, `key_value_store` | small |

- `note_actions.serialized_payload` is JSON holding `{actions: [{name, noteId, operations, …}]}`.
- The action names, by count:

  | Action | Count |
  |---|---|
  | `ReplaceStrokeAction` | 70,843 |
  | `InsertStrokeAction` | 42,465 |
  | `CreateNoteAction` | 2,481 |
  | `InsertShapeAction` | 1,837 |
  | `SetNoteTemplateAction` | 1,219 |
  | `DeleteShapeAction` | 820 |
  | `TransformStrokeAction` | 319 |
  | `UpdateContentFeatureAction` | 139 |
  | `UpdateShapeAction` | 125 |
  | `DeleteNoteAction` | 52 |
  | `SetNotebookMetadataAction` | 9 |
  | `SetNoteBookTemplateAction` | 1 |

- `SetNotebookMetadataAction` only ever sets `kindle_title_metadata/language`. Its value is the Ion string `en_US`.
- `notebook_content` holds Ion fragments, base64 in JSON, waiting to upload.
- No row mentions a title or a folder.

**The notebook itself**: `$490` is `kindle_title_metadata` with `book_id` (a cloud id such as `JIPOkzsT…`) and `language`. There is no title.

**`system/`**:
- `fmcache/fmcache.db` has `app_sessions`, `reading_sessions` and `records`, which are metrics.
- `Search Indexes/Index.db` isn't SQLite.
- `ksdk/profiles.db` holds profiles.
- None of them have notebook names.

So the titles and folders live in the Kindle's content catalog (`/var/local/cc.db` on Kindle firmware), which MTP doesn't expose. The app keeps local titles (rename) instead. `folio-cli device get <path> [dest]` copies any single file off the Kindle for this kind of investigation.

**Annotation notebooks.** On 2026-10-24 the device has no `!!PDOC!!` or `!!EBOK!!` folders, so there was nothing to decode. Sync already copies them into `annotations/` when they appear.

## 9. Handwriting on PDFs (`!!PDOC!!`), 2026-10-24

Seen on a Kindle Scribe after writing on all 3 pages of a test PDF (420×595 pt).

**Files on the Kindle**

| Path | What it holds |
|---|---|
| `.notebooks/<uuid>!!PDOC!!notebook/nbk` (+ `actions.log`, `nbk-journal`) | A normal NMDL notebook with the strokes. |
| `documents/<pdf stem>.sdr/<uuid>/` | `book.kdf`, `ManifestFile`, `book.sdr/*.yjr/.yjf`, `res/rsrc8` (0 bytes). |
| `documents/<pdf stem>.sdr/kpp_<uuid>/` | `book.kdf`, `ManifestFile`, `res/rsrc8`. |

About the `.sdr/<uuid>/` folder:
- **It links the PDF to the notebook.**
- Its `book.kdf` is the Kindle's own conversion of the PDF:
  - `on_device_conv_pdf`, `yj_fixed_layout`
  - `ASIN` = the uuid
  - 3 `$164` page images (420×595) pointing at `rsrc8`
- mtp-rs can't parse the object info of `rsrc8`, so sync writes empty files without asking the Kindle.

**Notebook contents**: 4 sections in reading order.
- The first section has no placement and no strokes. It's created with the notebook.
- The other three carry the canvas placement on the PDF page (`$306: $314`, percent):

  | Field | Value |
  |---|---|
  | `$56` width | 106.25 |
  | `$57` height | 100 |
  | `$58` **top** | 0 |
  | `$59` **left** | −3.125 |

- The canvas is always 15624×20832 (3:4). A page narrower than 3:4 gets a canvas wider than the page, centred: 0.75 / (420/595) = 1.0625.

**Page numbers**: no page number was found anywhere:
- not in the sections
- not in the sync DB (PDOC notebooks aren't queued there)
- not in the `.yjr`/`.yjf`/`book.kdf` sidecars

The app maps placed, non-empty sections in reading order to PDF pages 1, 2, 3… (`folio_core::annotations::map_pages`). This is right for the sample, which was written on pages 1→3 in order. **Unverified**: what happens when pages are written out of order or skipped. A second test PDF ("Scribe Library page order test", 5 pages, write on 4 then 2) is on the device for that.

**Rendering**
- In the app, the overlay is the notebook page rendered as ink-only PNG (grey + alpha), drawn on the PDF page canvas inside the placement box.
- In the export (`export_annotated`), lopdf wraps the page's own content in `q … Q` and appends the strokes as vector paths. The transform from canvas to points is `[w·pw/cw, 0, 0, −h·ph/ch, x0 + left·pw, y1 − top·ph]`, where `w`, `h`, `left` and `top` are the placement values and `pw`, `ph` are the page width and height. Highlighters are drawn at 20% opacity.
