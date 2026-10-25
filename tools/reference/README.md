# Reference harness

Python scripts that run kfxlib and dump what it decodes from your own
notebooks. Folio's Rust decoders are tested against these dumps (the golden
tests in `crates/core/tests` and `crates/nmdl/tests`, which skip when
`fixtures/` is empty).

They need a checkout of the [scribe-library](https://github.com/technolaaji/scribe-library)
Python app, which carries kfxlib and
`nbk_to_pdf.py`. Point `KFXLIB_ROOT` at it and use its virtualenv:

```bash
KFXLIB_ROOT=~/src/scribe-library ~/src/scribe-library/.venv/bin/python tools/reference/dump_notebook.py path/to/notebook.nbk
```

| Script | What it does |
|---|---|
| `dump_notebook.py` | Dumps one or more notebooks to `fixtures/reference/<uuid>/` |
| `make_synthetic.py` | Builds a small notebook from a real one (first N pages) for unit tests |
| `check_reference.py` | Verifies every notebook has a complete dump and prints a table |
| `check_pdf.py` | Checks a `folio-cli convert` PDF with PyMuPDF and against the reference PNGs |

All output lands in `fixtures/`, which is git-ignored: it is made from your own notebooks.

## Output of `dump_notebook.py`

```
fixtures/reference/<uuid>/
  summary.json          fragment counts by type (after kfxlib's reachability check),
                        reading orders, page sections, canvas sizes, templates,
                        per-page template ids, stroke/point counts, brushes, chosen
                        pages, kfxlib log grouped by message type (long examples keep
                        their head and tail), timing
  model.json.gz         {"harness_version", "notebook", "pages": [page...]}
  templates/<type>.svg  template SVGs (1860×2480 space), e.g. dotted_1860x2480.svg
  pages/pNNNN.svg       stroke layer of each chosen page, vector mode
  png/pNNNN_vector.png  chosen page, vector-gray strokes, 1860×2480 grey
  png/pNNNN_raster.png  chosen page, pencil texture (PNG_SCALE_FACTOR 8), 1860×2480 grey
```

A page in `model.json.gz`:

```json
{"index": 1, "section": "c…", "canvas": [15624, 20832], "normalized_ppi": 2520.0,
 "template_id": null, "template_id_explicit": false, "inline_placement_type": null,
 "stroke_groups": 12,
 "strokes": [{
   "id": "i…", "brush_type": 5, "brush_name": "pencil", "color": 0, "thickness": 39.375,
   "seed": 123, "bounds": [x0, y0, x1, y1], "num_points": 42,
   "variable_density": true, "variable_thickness": false, "transform": null,
   "extra_fields": {"nmdl.origin_stroke_id": "…"}, "extra_point_fields": [],
   "arrays": {"position_x": […], "position_y": […], "density_adjust_factor": […],
              "thickness_adjust_factor": […], "tilt_x": […], "tilt_y": […], "pressure": […]},
   "points": [[x, y, t, d], …]
 }]}
```

- `arrays` are the raw `decode_stroke_values` output; `points` are the built points
  (docs/format.md §4.3: absolute coordinates, quantised thickness, duplicates dropped).
- Pages are in rendering order: reading-order sections that are page sections.
  Unreferenced (deleted) pages are not included.
- Chosen pages: 1, 2, the middle, the last, plus regression pages from PLAN.md
  (`55be9342` p83, `0da54486` p2) and any `--pages`.

## How it works

kfxlib's notebook methods are wrapped once (`install_hooks`) to record each page
section, template section and stroke before kfxlib consumes them. Two passes run per
notebook: a full vector-mode pass (model, SVGs, vector PNGs), then a raster pass in
which only the chosen pages get the slow pencil texture. The pencil PRNG is seeded
per stroke, so skipping other pages does not change the chosen ones.

PNGs are rendered with `nbk_to_pdf._rasterize_page` (cairosvg), i.e. exactly what the
Python app puts in its PDFs, converted to 8-bit grey.

## Synthetic notebook

`make_synthetic.py` removes the fingerprints (output is plain SQLite), trims the
reading order in both `document_data` and `metadata`, and deletes fragments only
the dropped pages reach. References between fragments are `$598::"id"` annotated
strings; stroke stories also point back at their page section, so reachability
does not follow edges into sections. The `local_action_fragments` edit log is
cleared. `fixtures/synthetic/two_pages.nbk` is built from `180b99f4…` (3 pages → 2).
