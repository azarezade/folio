# Development

## Requirements

- macOS 12+ with the Xcode command-line tools
- Rust, with the version and targets pinned in `rust-toolchain.toml`. `rustup` installs them on first use.
- Node 20+ and npm
- Python 3, for `tools/notices.py`

## Build and run

```bash
(cd app/ui && npm install && npm run build)   # the UI, into app/ui/dist
cargo build --release                         # the workspace, including folio-cli
app/build.sh                                  # Folio.app for this Mac
app/build.sh --universal                      # Folio.app + .dmg for Apple silicon and Intel
```

To point the app at a different library while developing:

```bash
FOLIO_LIBRARY=/path/to/library target/release/bundle/macos/Folio.app/Contents/MacOS/folio
```

### The dev bridge (UI in a browser)

`folio-cli serve` runs the real core behind HTTP and serves the built UI, with the same CSP as the app:

```bash
target/release/folio-cli serve --library /path/to/library    # http://127.0.0.1:8137
```

For hot reload, run `npm run dev` in `app/ui` (Vite proxies `/api`, `/folio` and `/events` to the bridge). The component gallery is at `/?gallery`.

The app uses WebKit and a browser may use another engine, so check layout in the app too.

## The command line

```bash
target/release/folio-cli --help
folio-cli inspect notebook.nbk --strokes        # fragments, pages, strokes, diagnostics
folio-cli convert notebook.nbk out.pdf --vector # PDF export (raster by default)
folio-cli render-page notebook.nbk 3 page.png
folio-cli device ls documents                   # browse the Kindle
folio-cli sync --library /path/to/library --dry-run
folio-cli push file.pdf
```

## Tests

```bash
cargo test --workspace --release
(cd app/ui && npx svelte-check)
cargo clippy --workspace --all-targets
```

The unit and integration tests run without any personal data. They use hand-built books, PDFs and Ion data, and a virtual MTP device for sync.

Two kinds of tests use your own data, and skip when it isn't there:

- **Golden tests** (`crates/core/tests/golden_*`, `crates/nmdl/tests/golden_model.rs`) compare every page and stroke with what kfxlib decodes. Create the reference dumps with `tools/reference/` (see its README); they go in the git-ignored `fixtures/`.
- **Library tests** (`graphics_on_canvas`, `pdf_annotations`, `books::user_library`) run over a real Folio library:

  ```bash
  FOLIO_TEST_LIBRARY=~/Library/Application\ Support/Folio cargo test --release
  ```

Never commit anything from `fixtures/` or a library. It's your own handwriting and books.

### Fuzzing

`fuzz/` is a separate workspace for nightly Rust, with targets for the Ion reader and the stroke codec:

```bash
cd fuzz && cargo +nightly fuzz run ion_read
```

## Format notes

[format.md](format.md) documents the KDF container, Ion, the notebook model, strokes, device layout, and the findings behind the decoders.
