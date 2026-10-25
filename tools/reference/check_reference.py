#!/usr/bin/env python3
"""
Check that every notebook has a complete reference dump (Phase 0's "done when"),
and print one line per notebook.

Usage:
    python3 tools/reference/check_reference.py [../library/notebooks] [--ref fixtures/reference]
"""

from __future__ import annotations

import argparse
import gzip
import json
import os
import sys
from pathlib import Path

NATIVE_ROOT = Path(__file__).resolve().parents[2]


def check(uuid: str, ref: Path) -> list[str]:
    d = ref / uuid
    if not (d / "summary.json").exists():
        return ["no summary.json"]
    s = json.loads((d / "summary.json").read_text())
    problems = []
    with gzip.open(d / "model.json.gz", "rt") as f:
        model = json.load(f)
    if len(model["pages"]) != s["page_count"]:
        problems.append(f"model has {len(model['pages'])} pages, summary {s['page_count']}")
    if sum(len(p["strokes"]) for p in model["pages"]) != s["stroke_count"]:
        problems.append("model stroke count differs from summary")
    for n in s["chosen_pages"]:
        for kind in ("vector", "raster"):
            if not (d / "png" / f"p{n:04d}_{kind}.png").exists():
                problems.append(f"missing p{n:04d}_{kind}.png")
    for t in s["templates"]:
        if not (d / "templates" / f"{t['template_type']}.svg").exists():
            problems.append(f"missing template {t['template_type']}.svg")
    return problems


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("notebooks", nargs="?", default=str(Path(os.environ.get("FOLIO_TEST_LIBRARY", NATIVE_ROOT.parent / "library")) / "notebooks"))
    ap.add_argument("--ref", default=str(NATIVE_ROOT / "fixtures" / "reference"))
    args = ap.parse_args()

    ref = Path(args.ref)
    uuids = sorted(p.stem for p in Path(args.notebooks).glob("*.nbk"))
    bad = 0
    print(f"{'notebook':10} {'pages':>6} {'strokes':>8} {'frags':>7} {'templates':>9} {'pngs':>5} {'secs':>6}  status")
    for uuid in uuids:
        problems = check(uuid, ref)
        s = json.loads((ref / uuid / "summary.json").read_text()) if (ref / uuid / "summary.json").exists() else {}
        pngs = len(list((ref / uuid / "png").glob("*.png"))) if s else 0
        print(f"{uuid[:8]:10} {s.get('page_count', '-'):>6} {s.get('stroke_count', '-'):>8} "
              f"{s.get('fragments', {}).get('total', '-'):>7} {len(s.get('templates', [])):>9} {pngs:>5} "
              f"{s.get('timing_s', {}).get('total', '-'):>6}  {'ok' if not problems else '; '.join(problems)}")
        bad += bool(problems)
    print(f"\n{len(uuids) - bad}/{len(uuids)} notebooks have complete reference dumps")
    sys.exit(1 if bad or not uuids else 0)


if __name__ == "__main__":
    main()
