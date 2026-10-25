#!/usr/bin/env python3
"""
Check PDFs written by `folio-cli convert` with an independent reader (PyMuPDF):
page count, page size, no repairs or MuPDF warnings, and, given the notebook's
reference dump, that the rendered chosen pages match the kfxlib reference PNGs.

Usage (with the parent repo's venv):
    ../.venv/bin/python tools/reference/check_pdf.py <out.pdf> [--reference fixtures/reference/<uuid>]
        [--expect-pages N]

Vector PDFs are compared with `pNNNN_vector.png`, raster ones with
`pNNNN_raster.png`, after a 4×4 box filter (textures are random).
"""

import argparse
import sys
from pathlib import Path

import numpy as np
import pymupdf
from PIL import Image

LONG_EDGE_PT = 2480 / 300 * 72
MAX_MAE = {"vector": 3.0, "raster": 5.0}


def boxed(img: Image.Image) -> np.ndarray:
    return np.asarray(img.resize((img.width // 4, img.height // 4), Image.BOX), dtype=float)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("pdf")
    ap.add_argument("--reference")
    ap.add_argument("--expect-pages", type=int)
    args = ap.parse_args()

    pymupdf.TOOLS.mupdf_display_errors(False)
    pymupdf.TOOLS.reset_mupdf_warnings()
    doc = pymupdf.open(args.pdf)
    problems = []
    if doc.is_repaired:
        problems.append("MuPDF had to repair the file")
    if args.expect_pages is not None and doc.page_count != args.expect_pages:
        problems.append(f"{doc.page_count} pages, expected {args.expect_pages}")
    for page in doc:
        if abs(max(page.rect.width, page.rect.height) - LONG_EDGE_PT) > 0.5:
            problems.append(f"page {page.number + 1} is {page.rect.width:.1f}×{page.rect.height:.1f} pt")
            break
    is_vector = not any(doc[0].get_images())
    kind = "vector" if is_vector else "raster"

    if args.reference:
        for png in sorted(Path(args.reference, "png").glob(f"p*_{kind}.png")):
            n = int(png.name[1:5])
            if n > doc.page_count:
                continue
            ref = Image.open(png).convert("L")
            page = doc[n - 1]
            # render at 4× and box down, so anti-aliasing differences average out
            zoom = 4 * ref.width / page.rect.width
            pix = page.get_pixmap(matrix=pymupdf.Matrix(zoom, zoom), colorspace=pymupdf.csGRAY)
            ours = Image.frombytes("L", (pix.width, pix.height), pix.samples).resize(ref.size, Image.BOX)
            mae = np.abs(boxed(ours) - boxed(ref)).mean()
            status = "ok" if mae <= MAX_MAE[kind] else "DIFFERS"
            print(f"  page {n:>4} vs {png.name}: MAE {mae:.2f} {status}")
            if mae > MAX_MAE[kind]:
                problems.append(f"page {n} differs from {png.name} (MAE {mae:.2f})")

    warnings = pymupdf.TOOLS.mupdf_warnings()
    if warnings:
        problems.append(f"MuPDF warnings: {warnings[:200]}")
    print(f"{args.pdf}: {doc.page_count} pages, {kind}, {'OK' if not problems else '; '.join(problems)}")
    sys.exit(1 if problems else 0)


if __name__ == "__main__":
    main()
