#!/usr/bin/env python3
"""
Build a small synthetic notebook for unit tests from a real one: keep the
first N pages of the reading order and delete every fragment that only the
dropped pages reference.

The output is plain SQLite (fingerprints removed), which is still a valid
notebook: kfxlib and the Rust port both skip fingerprint removal when the
signature is absent. Tables other than the fragment stores are kept as-is.

Usage (with the parent repo's venv):
    ../.venv/bin/python tools/reference/make_synthetic.py <in.nbk> <out.nbk> [--pages 2]
"""

from __future__ import annotations

import argparse
import shutil
import sqlite3
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_ROOT))

FINGERPRINT = b"\xfa\x50\x0a\x5f"
FRAGMENT_TABLES = ("fragments", "local_delta_fragments")


def remove_fingerprints(data: bytes) -> bytes:
    """Same rule as kfxlib's SQLiteFingerprintWrapper (docs/format.md §1.1)."""
    if data[1024:1028] != FINGERPRINT:
        return data
    out = bytearray(data)
    offset = 1024
    while len(out) >= offset + 1024:
        if out[offset:offset + 4] != FINGERPRINT:
            raise ValueError(f"missing fingerprint at {offset}")
        del out[offset:offset + 1024]
        offset += 1024 * 1024
    return bytes(out)


def ref_id(value) -> str | None:
    """Fragment references in KDF values are $598-annotated strings (kfxid)."""
    from kfxlib.ion import IonAnnotation

    if isinstance(value, IonAnnotation) and value.is_annotation("$598") and isinstance(value.value, str):
        return value.value
    return None


def symbols_in(value, found: set[str]):
    from kfxlib.ion import IonAnnotation, IonSymbol

    if ref_id(value) is not None:
        found.add(ref_id(value))
    elif isinstance(value, IonAnnotation):
        symbols_in(value.value, found)
    elif isinstance(value, IonSymbol):
        found.add(str(value))
    elif isinstance(value, dict):
        for v in value.values():
            symbols_in(v, found)
    elif isinstance(value, (list, tuple)):
        for v in value:
            symbols_in(v, found)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("input")
    ap.add_argument("output")
    ap.add_argument("--pages", type=int, default=2)
    args = ap.parse_args()

    from kfxlib.ion import IonAnnotation
    from kfxlib.ion_binary import IonBinary
    from kfxlib.message_logging import set_logger
    from kfxlib.yj_book import YJ_Book
    import logging
    set_logger(logging.getLogger("kfxlib"))
    logging.getLogger("kfxlib").addHandler(logging.NullHandler())
    logging.getLogger("kfxlib").propagate = False

    out = Path(args.output)
    out.write_bytes(remove_fingerprints(Path(args.input).read_bytes()))

    # Let kfxlib build the symbol table (YJ_symbols import + the notebook's local symbols).
    tmp = Path(tempfile.mkdtemp(prefix="nbk_syn_"))
    try:
        shutil.copyfile(out, tmp / "nbk")
        book = YJ_Book(str(tmp))
        book.decode_book()
        symtab = book.symtab
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    ion = IonBinary(symtab)
    db = sqlite3.connect(out)

    rows = {}       # id -> (table, payload); delta rows override fragments rows
    for table in reversed(FRAGMENT_TABLES):
        for fid, ptype, payload in db.execute(f"SELECT id, payload_type, payload_value FROM {table}"):
            rows[fid] = (table, ptype, bytes(payload))

    def decode(fid):
        _, ptype, payload = rows[fid]
        if ptype != "blob" or not payload.startswith(IonBinary.SIGNATURE) or len(payload) == 4:
            return None
        if fid in ("$ion_symbol_table", "max_id"):
            return None
        return ion.deserialize_single_value(payload)

    refs = {}
    for fid in rows:
        found = set()
        symbols_in(decode(fid), found)
        refs[fid] = {s for s in found if s in rows and s != fid}

    # document_data and metadata each hold a copy of the reading orders ($169)
    order_holders = [f for f in ("document_data", "metadata") if f in rows]
    docs = {f: decode(f) for f in order_holders}
    reading_order = docs["document_data"].value["$169"][0]["$170"]
    kept = [ref_id(s) for s in reading_order[:args.pages]]
    dropped = [ref_id(s) for s in reading_order[args.pages:]]
    if not dropped:
        sys.exit(f"notebook has only {len(reading_order)} pages; nothing to drop")
    for doc in docs.values():
        assert isinstance(doc, IonAnnotation)
        for ro in doc.value.get("$169", []):
            ro["$170"][:] = [s for s in ro["$170"] if ref_id(s) not in dropped]

    # Content fragments link back to their page section, so edges into a section are
    # not followed: a page's tree is what its section reaches without passing through
    # another section.
    sections = set(kept) | set(dropped)

    def closure(start):
        seen, stack = set(), list(start)
        while stack:
            f = stack.pop()
            if f not in seen:
                seen.add(f)
                stack.extend(r for r in refs.get(f, ()) if r not in sections)
        return seen

    content = closure(sections)
    roots = [f for f in rows if f not in content and f not in order_holders] + kept
    keep = closure(roots) | set(order_holders)
    delete = sorted(set(rows) - keep)

    for fid, doc in docs.items():
        db.execute(f"UPDATE {rows[fid][0]} SET payload_value = ? WHERE id = ?", (ion.serialize_single_value(doc), fid))
    for fid in delete:
        for t in FRAGMENT_TABLES:
            db.execute(f"DELETE FROM {t} WHERE id = ?", (fid,))
        db.execute("DELETE FROM fragment_properties WHERE id = ?", (fid,))
    db.execute("DELETE FROM local_action_fragments")     # edit log; would name deleted strokes
    db.commit()
    db.execute("VACUUM")
    db.close()

    print(f"{out}: kept {len(kept)} pages, deleted {len(delete)} of {len(rows)} fragments, "
          f"{out.stat().st_size} bytes")


if __name__ == "__main__":
    main()
