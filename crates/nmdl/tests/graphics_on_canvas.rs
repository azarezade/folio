//! "Convert to shape" graphics must overlap their page: `$59` is left and `$58`
//! is top. Read the other way round, shapes landed off the canvas (19567df8
//! page 4 had a box at x = 17455 on a 15624-wide page). Runs over the
//! notebooks in `$FOLIO_TEST_LIBRARY/notebooks`; without it the test passes with a note.

use std::path::Path;

use folio_kdf::OpenOptions;
use folio_kdf::ion::Diagnostics;
use folio_nmdl::Notebook;

#[test]
fn graphics_overlap_the_canvas() {
    let Some(dir) = std::env::var_os("FOLIO_TEST_LIBRARY").map(|l| Path::new(&l).join("notebooks")) else {
        eprintln!("skipped: set FOLIO_TEST_LIBRARY to a Folio library folder");
        return;
    };
    let entries = std::fs::read_dir(&dir).expect("FOLIO_TEST_LIBRARY/notebooks");
    let (mut checked, mut off) = (0usize, Vec::new());
    for e in entries {
        let path = e.unwrap().path();
        if path.extension().is_none_or(|x| x != "nbk") {
            continue;
        }
        let mut d = Diagnostics::default();
        let nb = Notebook::open(&path, &OpenOptions::default(), &mut d).unwrap();
        for i in 0..nb.page_count() {
            let page = nb.page(i, &mut d).unwrap();
            let (w, h) = (page.canvas.0 as f64, page.canvas.1 as f64);
            for g in &page.graphics {
                checked += 1;
                // boxes may hang over an edge, but should overlap the page
                if !(g.left < w && g.top < h && g.left + g.width > 0.0 && g.top + g.height > 0.0) {
                    let name = path.file_stem().unwrap().to_string_lossy()[..8].to_string();
                    off.push(format!("{name} p{} ({:.0}, {:.0})", i + 1, g.left, g.top));
                }
            }
        }
    }
    eprintln!("{checked} graphics checked, {} off the page: {off:?}", off.len());
    // One shape in a test library really is above its page (a line
    // dragged off the top); transposed, dozens were.
    assert!(
        off.len() * 100 <= checked.max(1),
        "{} of {checked} graphics off the page: {off:?}",
        off.len()
    );
}
