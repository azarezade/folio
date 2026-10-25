//! Handwriting on a PDF: for every PDF in `$FOLIO_TEST_LIBRARY` that the
//! Kindle has written on (a `<stem>.sdr/<uuid>/` folder and an
//! `annotations/<uuid>!!PDOC!!notebook.nbk`), the pages map to PDF pages, the
//! overlay renders, and the export keeps the text and adds ink to each
//! annotated page. It indexes that library as the app does; the page cache
//! goes to a temporary folder. Without the variable the test passes with a note.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use folio_core::app::App;
use folio_library::Layout;
use folio_library::cache::Cache;

fn pdfs(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() && !p.extension().is_some_and(|x| x == "sdr") {
            pdfs(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")) {
            out.push(p);
        }
    }
}

#[test]
fn overlay_and_export() {
    let Some(src) = std::env::var_os("FOLIO_TEST_LIBRARY").map(PathBuf::from) else {
        eprintln!("skipped: set FOLIO_TEST_LIBRARY to a Folio library folder");
        return;
    };
    let mut found = Vec::new();
    pdfs(&src.join("books"), &mut found);
    let annotated: Vec<PathBuf> = found
        .into_iter()
        .filter(|p| folio_core::annotations::notebook_path(&src, p).is_some())
        .collect();
    if annotated.is_empty() {
        eprintln!("skipped: no PDF with Kindle handwriting in {}", src.display());
        return;
    }

    // a scratch cache, so the test never writes into the real one
    let cache = std::env::temp_dir().join(format!("folio-annot-cache-{}", std::process::id()));
    let app: Arc<App> = App::new(Layout::new(src.clone()), Cache::new(cache.clone()), Arc::new(|_| {})).unwrap();
    let items = app.with_index(|i| i.items(None)).unwrap();
    for pdf in &annotated {
        let rel = pdf.strip_prefix(&src).unwrap().to_string_lossy().to_string();
        let book = items.iter().find(|i| i.rel_path == rel).expect("indexed");

        let pages = folio_core::annotations::pages(&app, &book.id).unwrap();
        assert!(!pages.is_empty(), "{rel}: handwriting found but no placed pages");
        for p in &pages {
            let pl = p.placement;
            assert!(pl.width > 0.5 && pl.height > 0.5, "{rel}: odd placement {pl:?}");
        }
        let png = folio_core::annotations::overlay_png(&app, &book.id, pages[0].notebook_page, 512).unwrap();
        assert_eq!(&png[..4], b"\x89PNG");

        let out = cache.join("annotated.pdf");
        let n = folio_core::annotations::export_pdf(&app, &book.id, &out).unwrap();
        assert_eq!(n, pages.len(), "{rel}: every annotated page gets ink");
        let doc = lopdf::Document::load(&out).unwrap();
        let ids = doc.get_pages();
        for p in &pages {
            let id = ids[&(p.pdf_page as u32 + 1)];
            assert!(doc.get_page_contents(id).len() >= 3, "{rel}: q, the page, the ink");
        }
        // the original text survives
        let before = folio_kfx::text::extract(pdf).unwrap();
        let after = folio_kfx::text::extract(&out).unwrap();
        assert_eq!(before.len(), after.len(), "{rel}: text pages kept");
    }
    let _ = std::fs::remove_dir_all(&cache);
}
