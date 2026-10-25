#!/usr/bin/env python3
"""
Reference harness: dump what kfxlib (in the parent repo) decodes from a Scribe
notebook, so the Rust port can be checked against it.

For each notebook it writes, under <out>/<uuid>/:

    summary.json        fragment counts by type, reading orders, sections,
                        page count, templates, chosen pages, kfxlib log summary
    model.json.gz       per page: canvas, template id and every stroke
                        (brush, colour, thickness, bounds, seed, raw point
                        arrays and the built (x, y, t, d) points)
    templates/<type>.svg  template SVGs as kfxlib renders them
    pages/pNNNN.svg     the stroke layer of each chosen page (vector mode)
    png/pNNNN_vector.png  chosen pages rendered with vector-gray strokes
    png/pNNNN_raster.png  chosen pages rendered with the pencil texture

Two kfxlib passes run per notebook: a full one in vector mode (model, SVGs,
vector PNGs), then a raster one where only the chosen pages get the slow
pencil-texture treatment. The PRNG is seeded per stroke, so skipping the
texture elsewhere does not change the chosen pages.

Usage (with the parent repo's venv):
    ../.venv/bin/python tools/reference/dump_notebook.py <nbk or dir>... [--out fixtures/reference]
        [--pages 1,83] [--jobs N] [--force]
"""

from __future__ import annotations

import argparse
import copy
import gzip
import io
import json
import logging
import re
import shutil
import subprocess
import os
import sys
import tempfile
import time
import traceback
import zipfile
from collections import Counter
from pathlib import Path

HARNESS_VERSION = 2

# The folder holding kfxlib/ and nbk_to_pdf.py (the scribe-library Python app).
REPO_ROOT = Path(os.environ.get("KFXLIB_ROOT", Path(__file__).resolve().parents[3]))
NATIVE_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT))

# Pages the plan calls out as regression cases, by uuid prefix.
EXTRA_PAGES = {
    "55be9342": [83],   # thin pencil strokes that rendered dotted
    "0da54486": [2],    # the portrait
}

STROKE_ARRAYS = [
    "nmdl.position_x", "nmdl.position_y", "nmdl.density_adjust_factor", "nmdl.thickness_adjust_factor",
    "nmdl.tilt_x", "nmdl.tilt_y", "nmdl.pressure"]


def plain(v):
    """Convert kfxlib Ion values to JSON-able Python values."""
    from kfxlib.ion import IonAnnotation, IonBLOB, IonSymbol

    if isinstance(v, IonAnnotation):
        return {"annotations": [str(a) for a in v.annotations], "value": plain(v.value)}
    if isinstance(v, IonSymbol):
        return str(v)
    if isinstance(v, (bytes, bytearray, IonBLOB)):
        return {"blob_len": len(v)}
    if isinstance(v, dict):
        return {str(k): plain(x) for k, x in v.items()}
    if isinstance(v, (list, tuple)):
        return [plain(x) for x in v]
    if isinstance(v, bool) or v is None or isinstance(v, (int, str)):
        return v
    if isinstance(v, float):
        return v
    return str(v)


def normalise_message(msg: str) -> str:
    """Collapse a log line to its type, so thousands of variants count as one."""
    msg = re.sub(r"^section .*?(?= (content type|has |nmdl|point|Unexpected|unexpected))", "…", msg)    # drop the context path
    msg = re.sub(r"\{\s*([\w.$]+):.*\}", r"{\1: …}", msg)
    msg = re.sub(r"\[.*\]", "[…]", msg)
    msg = re.sub(r"\$\d+|\b\w*\d\w*\b|-?\d+(\.\d+)?", "#", msg)
    return msg[:160]


class Recorder:
    """Records what kfxlib's notebook decoder sees, via the hooks in install_hooks()."""

    def __init__(self, chosen: set[int] | None, record: bool):
        self.chosen = chosen          # 1-based page numbers that get full rendering (raster pass)
        self.record = record
        self.pages: list[dict] = []
        self.templates: list[dict] = []
        self.sections: list[dict] = []
        self.page = None

    def record_stroke(self, content, location_id):
        from kfxlib import yj_to_epub_notebook as nb

        nmdl_type = str(content.get("nmdl.type"))
        if nmdl_type == "nmdl.stroke_group":
            self.page["stroke_groups"] += 1
            return
        if nmdl_type != "nmdl.stroke":
            self.page["strokes"].append({"id": plain(location_id), "nmdl_type": nmdl_type})
            return

        content = copy.deepcopy(content)
        sp = content.get("nmdl.stroke_points", {})
        num_points = sp.get("nmdl.num_points", 0)
        bounds = list(content.get("nmdl.stroke_bounds"))
        thickness = float(content.get("nmdl.thickness"))

        arrays = {}
        for name in STROKE_ARRAYS:
            if name in sp:
                vals = nb.decode_stroke_values(sp[name], num_points, name)
                arrays[name.removeprefix("nmdl.")] = vals
        extra = sorted(str(k) for k in sp.keys() if k not in STROKE_ARRAYS and k != "nmdl.num_points")

        daf = arrays.get("density_adjust_factor") or []
        taf = arrays.get("thickness_adjust_factor") or []
        variable_density = any(v != 100 for v in daf)
        variable_thickness = any(v != 100 for v in taf)

        # Same point construction as yj_to_epub_notebook.scribe_notebook_stroke
        points = []
        last_x = last_y = None
        xs, ys = arrays.get("position_x") or [], arrays.get("position_y") or []
        for i in range(num_points):
            x = xs[i] + bounds[0]
            y = ys[i] + bounds[1]
            t_af = taf[i] if variable_thickness else 100
            d_af = daf[i] if variable_density else 100
            if nb.QUANTIZE_THICKNESS:
                t_af = (t_af // 10) * 10
            t = round(thickness * t_af / 100.0)
            d = d_af / 100.0
            if x != last_x or y != last_y:
                points.append([x, y, t, d])
            last_x, last_y = x, y

        brush = content.get("nmdl.brush_type")
        brush_name = {0: nb.ORIGINAL_PEN, 1: nb.HIGHLIGHTER, 5: nb.PENCIL, 6: nb.FOUNTAIN_PEN}.get(brush)
        if brush == 7:
            brush_name = nb.MARKER if variable_thickness else nb.PEN
        if brush_name is None:
            brush_name = nb.UNKNOWN + str(brush)

        known = {"nmdl.type", "nmdl.brush_type", "nmdl.color", "nmdl.random_seed", "nmdl.stroke_bounds",
                 "nmdl.thickness", "nmdl.stroke_points", "$98", "$159"}

        self.page["strokes"].append({
            "id": plain(location_id),
            "brush_type": brush,
            "brush_name": brush_name,
            "color": content.get("nmdl.color"),
            "thickness": thickness,
            "seed": content.get("nmdl.random_seed"),
            "bounds": bounds,
            "num_points": num_points,
            "variable_density": variable_density,
            "variable_thickness": variable_thickness,
            "transform": plain(content.get("$98")),
            "extra_fields": {str(k): plain(v) for k, v in content.items() if str(k) not in known},
            "extra_point_fields": extra,
            "arrays": arrays,
            "points": points,
        })

CURRENT: Recorder | None = None
_HOOKED = False


def install_hooks():
    """Wrap kfxlib's notebook methods once; they report to the CURRENT recorder."""
    global _HOOKED
    if _HOOKED:
        return
    _HOOKED = True

    from kfxlib import yj_to_epub_notebook as nb
    from kfxlib.yj_to_epub import KFX_EPUB

    orig_section = KFX_EPUB.process_section
    orig_page = KFX_EPUB.process_scribe_notebook_page_section
    orig_template = KFX_EPUB.process_scribe_notebook_template_section
    orig_stroke = KFX_EPUB.scribe_notebook_stroke

    def process_section(self, section, seq):
        CURRENT.sections.append({
            "name": str(section.get("$174")),
            "kind": "page" if "nmdl.canvas_width" in section
                    else "template" if "nmdl.template_type" in section else "other",
        })
        return orig_section(self, section, seq)

    def page_section(self, section, page_template, section_name, seq):
        rec = CURRENT
        page = {
            "index": len(rec.pages) + 1,
            "section": str(section_name),
            "canvas": [section.get("nmdl.canvas_width"), section.get("nmdl.canvas_height")],
            "normalized_ppi": section.get("nmdl.normalized_ppi"),
            "template_id": plain(section.get("nmdl.template_id", self.nmdl_template_id)),
            "template_id_explicit": "nmdl.template_id" in section,
            "inline_placement_type": plain(section.get("nmdl.inline_placement_type")),
            "stroke_groups": 0,
            "strokes": [],
        }
        rec.pages.append(page)
        rec.page = page

        saved = nb.VECTOR_VARIABLE_DENSITY
        if rec.chosen is not None and page["index"] not in rec.chosen:
            nb.VECTOR_VARIABLE_DENSITY = True     # skip the slow pencil texture on pages we won't render
        try:
            return orig_page(self, section, page_template, section_name, seq)
        finally:
            nb.VECTOR_VARIABLE_DENSITY = saved
            rec.page = None

    def template_section(self, section, page_template, section_name):
        CURRENT.templates.append({
            "section": str(section_name),
            "template_type": str(section.get("nmdl.template_type")),
        })
        return orig_template(self, section, page_template, section_name)

    def stroke(self, content, parent, location_id):
        if CURRENT.record and CURRENT.page is not None:
            CURRENT.record_stroke(content, location_id)
        return orig_stroke(self, content, parent, location_id)

    KFX_EPUB.process_section = process_section
    KFX_EPUB.process_scribe_notebook_page_section = page_section
    KFX_EPUB.process_scribe_notebook_template_section = template_section
    KFX_EPUB.scribe_notebook_stroke = stroke


def run_kfxlib(nbk_path: Path, recorder: Recorder, vector: bool):
    """Run kfxlib on one notebook. Returns (epub bytes, fragment summary, reading orders, job log)."""
    global CURRENT
    from kfxlib import yj_to_epub_notebook as nb
    from kfxlib.message_logging import JobLog, set_logger
    from kfxlib.yj_book import YJ_Book

    logger = logging.getLogger("kfxlib")
    logger.setLevel(logging.INFO)
    logger.propagate = False
    if not logger.handlers:
        logger.addHandler(logging.NullHandler())     # collected in the summary instead
    job_log = JobLog(logger)
    set_logger(job_log)
    nb.VECTOR_VARIABLE_DENSITY = vector

    tmpdir = Path(tempfile.mkdtemp(prefix="nbk_ref_"))
    try:
        staged_dir = tmpdir / "notebook"
        staged_dir.mkdir()
        shutil.copyfile(nbk_path, staged_dir / "nbk")     # kfxlib's directory mode looks for a file named "nbk"

        book = YJ_Book(str(staged_dir))
        book.decode_book()
        by_type = Counter(str(f.ftype) for f in book.fragments)
        fragments = {"total": len(book.fragments), "by_type": dict(sorted(by_type.items()))}
        reading_orders = []
        for f in book.fragments:
            if str(f.ftype) == "$538":       # document_data holds the reading orders
                for ro in f.value.get("$169", []):
                    reading_orders.append({
                        "name": plain(ro.get("$178")),
                        "sections": [str(s) for s in ro.get("$170", [])],
                    })

        install_hooks()
        CURRENT = recorder
        epub = book.convert_to_epub()
        return epub, fragments, reading_orders, job_log
    finally:
        set_logger(None)
        shutil.rmtree(tmpdir, ignore_errors=True)


def epub_svgs(epub: bytes) -> dict[str, bytes]:
    with zipfile.ZipFile(io.BytesIO(epub)) as zf:
        return {n.rsplit("/", 1)[-1]: zf.read(n) for n in zf.namelist() if n.lower().endswith(".svg")}


def render_pngs(epub: bytes, chosen: list[int], out_dir: Path, suffix: str) -> int:
    import nbk_to_pdf

    specs = nbk_to_pdf._epub_pages(epub)
    for n in chosen:
        if n <= len(specs):
            img = nbk_to_pdf._rasterize_page(specs[n - 1][1])
            img.convert("L").save(out_dir / f"p{n:04d}_{suffix}.png", optimize=True)
    return len(specs)


def choose_pages(uuid: str, page_count: int, extra: list[int]) -> list[int]:
    pages = {1, 2, (page_count + 1) // 2, page_count}
    for prefix, nums in EXTRA_PAGES.items():
        if uuid.startswith(prefix):
            pages.update(nums)
    pages.update(extra)
    return sorted(p for p in pages if 1 <= p <= page_count)


def shorten(msg: str) -> str:
    """Keeps the head and tail of long messages (kfxlib puts totals like "(N total)" at the end)."""
    return msg if len(msg) <= 300 else msg[:240] + " … " + msg[-56:]


def log_summary(job_log) -> dict:
    out = {}
    for level, msgs in (("errors", job_log.errors), ("warnings", job_log.warnings)):
        c = Counter(normalise_message(m) for m in msgs)
        out[level] = [{"message": m, "count": n, "example": shorten(next(x for x in msgs if normalise_message(x) == m))}
                      for m, n in c.most_common()]
    return out


def dump_one(nbk_path: Path, out_root: Path, extra_pages: list[int]) -> dict:
    uuid = nbk_path.stem
    out = out_root / uuid
    if out.exists():
        shutil.rmtree(out)
    for sub in ("templates", "pages", "png"):
        (out / sub).mkdir(parents=True)

    t0 = time.monotonic()
    rec = Recorder(chosen=None, record=True)
    epub, fragments, reading_orders, job_log = run_kfxlib(nbk_path, rec, vector=True)
    t_decode = time.monotonic() - t0

    page_count = len(rec.pages)
    chosen = choose_pages(uuid, page_count, extra_pages)

    svgs = epub_svgs(epub)
    template_types = {t["template_type"] for t in rec.templates}
    for name, data in svgs.items():
        if name.removesuffix(".svg") in template_types:
            (out / "templates" / name).write_bytes(data)
    for n in chosen:
        name = f"{rec.pages[n - 1]['section']}.svg"
        if name in svgs:
            (out / "pages" / f"p{n:04d}.svg").write_bytes(svgs[name])

    spine_pages = render_pngs(epub, chosen, out / "png", "vector")
    if spine_pages != page_count:
        raise RuntimeError(f"EPUB has {spine_pages} pages but kfxlib processed {page_count} page sections")
    t_vector = time.monotonic() - t0

    raster_rec = Recorder(chosen=set(chosen), record=False)
    raster_epub, _, _, _ = run_kfxlib(nbk_path, raster_rec, vector=False)
    render_pngs(raster_epub, chosen, out / "png", "raster")
    t_total = time.monotonic() - t0

    model = {
        "harness_version": HARNESS_VERSION,
        "notebook": uuid,
        "pages": rec.pages,
    }
    with gzip.open(out / "model.json.gz", "wt", encoding="utf-8", compresslevel=6) as f:
        json.dump(model, f, separators=(",", ":"))

    brushes = Counter(s.get("brush_name") for p in rec.pages for s in p["strokes"])
    summary = {
        "harness_version": HARNESS_VERSION,
        "notebook": uuid,
        "source": str(nbk_path),
        "source_size": nbk_path.stat().st_size,
        "fragments": fragments,
        "reading_orders": reading_orders,
        "sections": {
            "processed": len(rec.sections),
            "by_kind": dict(Counter(s["kind"] for s in rec.sections)),
        },
        "page_count": page_count,
        "page_sections": [p["section"] for p in rec.pages],
        "canvas_sizes": dict(Counter("%sx%s" % tuple(p["canvas"]) for p in rec.pages)),
        "templates": rec.templates,
        "page_template_ids": dict(Counter(str(p["template_id"]) for p in rec.pages)),
        "stroke_count": sum(len(p["strokes"]) for p in rec.pages),
        "point_count": sum(s.get("num_points", 0) for p in rec.pages for s in p["strokes"]),
        "brushes": dict(brushes),
        "chosen_pages": chosen,
        "kfxlib_log": log_summary(job_log),
        "timing_s": {"vector_pass": round(t_vector, 1), "decode": round(t_decode, 1), "total": round(t_total, 1)},
    }
    (out / "summary.json").write_text(json.dumps(summary, indent=1))
    return summary


def find_notebooks(paths: list[str]) -> list[Path]:
    found = []
    for p in map(Path, paths):
        found.extend(sorted(p.glob("*.nbk")) if p.is_dir() else [p])
    return found


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("inputs", nargs="+", help=".nbk files or folders of them")
    ap.add_argument("--out", default=str(NATIVE_ROOT / "fixtures" / "reference"))
    ap.add_argument("--pages", default="", help="extra 1-based pages to render, comma-separated")
    ap.add_argument("--jobs", type=int, default=1, help="notebooks to process in parallel (one process each)")
    ap.add_argument("--force", action="store_true", help="redo notebooks whose summary.json already exists")
    args = ap.parse_args()

    logging.basicConfig(level=logging.WARNING, format="%(message)s")
    out_root = Path(args.out)
    out_root.mkdir(parents=True, exist_ok=True)
    extra = [int(x) for x in args.pages.split(",") if x.strip()]
    notebooks = find_notebooks(args.inputs)

    todo = [nb for nb in notebooks if args.force or not (out_root / nb.stem / "summary.json").exists()]
    for nb in notebooks:
        if nb not in todo:
            print(f"skip  {nb.stem} (already dumped)")

    if len(todo) == 1 or args.jobs <= 1:
        failed = 0
        for nb in todo:
            try:
                s = dump_one(nb, out_root, extra)
                print(f"ok    {nb.stem}: {s['page_count']} pages, {s['stroke_count']} strokes, "
                      f"{s['fragments']['total']} fragments, {s['timing_s']['total']} s", flush=True)
            except Exception:
                failed += 1
                print(f"FAIL  {nb.stem}\n{traceback.format_exc()}", flush=True)
        sys.exit(1 if failed else 0)

    # One child process per notebook: kfxlib keeps module-level state.
    cmd = [sys.executable, __file__, "--out", str(out_root), "--force"] + (["--pages", args.pages] if args.pages else [])
    running: list[subprocess.Popen] = []
    failed = 0
    queue = sorted(todo, key=lambda p: -p.stat().st_size)     # largest first
    while queue or running:
        while queue and len(running) < args.jobs:
            running.append(subprocess.Popen(cmd + [str(queue.pop(0))]))
        time.sleep(0.5)
        for proc in [p for p in running if p.poll() is not None]:
            running.remove(proc)
            failed += proc.returncode != 0
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
