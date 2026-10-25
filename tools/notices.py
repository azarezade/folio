#!/usr/bin/env python3
"""Writes app/src-tauri/THIRD_PARTY_NOTICES.txt: the licence of every crate
linked into the app (from `cargo metadata`), every npm package bundled into
the UI, and the vendored foliate-js. Run by app/build.sh before bundling."""
import json, os, subprocess, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "app/src-tauri/THIRD_PARTY_NOTICES.txt"
UI = ROOT / "app/ui"
LICENCE_NAMES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE")

def licence_texts(d: Path):
    found = []
    for f in sorted(d.iterdir()) if d.is_dir() else []:
        if f.is_file() and f.name.upper().startswith(LICENCE_NAMES):
            found.append((f.name, f.read_text(errors="replace").strip()))
    return found

def crates():
    meta = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--locked",
         "--filter-platform", "aarch64-apple-darwin"], cwd=ROOT))
    pkgs = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    root = next(i for i, p in pkgs.items() if p["name"] == "folio-app")
    seen, stack = set(), [root]
    while stack:  # normal (not dev or build) dependencies of the app
        i = stack.pop()
        if i in seen:
            continue
        seen.add(i)
        for d in nodes[i]["deps"]:
            if any(k["kind"] in (None, "normal") for k in d["dep_kinds"]):
                stack.append(d["pkg"])
    out = []
    for i in seen:
        p = pkgs[i]
        if p["source"] is None:  # our own crates
            continue
        out.append((p["name"], p["version"], p.get("license") or "see files",
                    licence_texts(Path(p["manifest_path"]).parent)))
    return sorted(out)

def npm():
    pkg = json.loads((UI / "package.json").read_text())
    names = list(pkg["dependencies"]) + ["svelte"]  # svelte's runtime is compiled in
    out = []
    for n in names:
        d = UI / "node_modules" / n
        meta = json.loads((d / "package.json").read_text())
        out.append((n, meta["version"], meta.get("license", "see files"), licence_texts(d)))
    return sorted(out)

ZIPJS = """Copyright (c) 2023 Gildas Lormeau. All rights reserved.

Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:
1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.
3. The names of the authors may not be used to endorse or promote products derived from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED ``AS IS'' AND ANY EXPRESSED OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL JCRAFT, INC. OR ANY CONTRIBUTORS TO THIS SOFTWARE BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE."""

FFLATE = """MIT License

Copyright (c) 2023 Arjun Barrett

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE."""

def main():
    foliate = UI / "src/vendor/foliate-js"
    vendored = [
        ("foliate-js", "78914ae", "MIT", licence_texts(foliate)),
        ("zip.js (in foliate-js)", "vendored", "BSD-3-Clause", [("LICENSE", ZIPJS)]),
        ("fflate (in foliate-js)", "vendored", "MIT", [("LICENSE", FFLATE)]),
    ]
    sections = [("Rust crates", crates()), ("JavaScript packages", npm()), ("Vendored code", vendored)]
    lines = [
        "Folio — third-party notices",
        "",
        "Folio is free software under the GNU General Public License,",
        "version 3 or later (see LICENSE). Its notebook and book decoding ports",
        "kfxlib by John Howell (GPLv3). It includes the following components,",
        "each under its own licence, reproduced below.",
        "",
    ]
    for title, items in sections:
        lines += ["=" * 78, title, "=" * 78, ""]
        lines += [f"  {n} {v} — {l}" for n, v, l, _ in items] + [""]
    texts = {}  # identical licence texts are printed once
    for _, items in sections:
        for n, v, l, files in items:
            for fname, text in files:
                texts.setdefault(text, []).append(f"{n} {v} ({fname})")
    lines += ["=" * 78, "Licence texts", "=" * 78, ""]
    for text, who in texts.items():
        lines += ["-" * 78, "Applies to: " + ", ".join(who), "-" * 78, "", text, ""]
    OUT.write_text("\n".join(lines))
    n = sum(len(i) for _, i in sections)
    missing = [f"{n} {v}" for _, items in sections for n, v, _, f in items if not f]
    print(f"{OUT.relative_to(ROOT)}: {n} components, {len(texts)} distinct licence texts")
    if missing:
        print("no licence file shipped (licence named in metadata only):", ", ".join(missing))

main()
