//! Handwriting on PDFs (Phase 8). Writing on a sideloaded PDF makes the
//! Kindle create a notebook `.notebooks/<uuid>!!PDOC!!notebook/nbk` (synced to
//! `annotations/`) and a `<pdf stem>.sdr/<uuid>/` folder next to the PDF,
//! which is how the two are linked. Each annotation page carries a
//! [`Placement`] of its canvas on the PDF page (docs/format.md §9).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use folio_kdf::ion::Diagnostics;
use folio_library::cache::width_bucket;
use folio_library::index::ItemKind;
use folio_nmdl::{Notebook, Placement};
use folio_render::{RENDERER_VERSION, render_page};
use serde::Serialize;

use crate::app::{App, Result};

/// One annotated PDF page.
#[derive(Clone, Debug, Serialize)]
pub struct AnnotatedPage {
    /// PDF page index, from 0.
    pub pdf_page: usize,
    /// Page index in the annotation notebook, from 0 (for the overlay image).
    pub notebook_page: usize,
    pub placement: Placement,
    pub strokes: usize,
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.char_indices().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

/// The annotation notebook for a book file, if the Kindle made one.
pub fn notebook_path(root: &Path, book: &Path) -> Option<PathBuf> {
    let stem = book.file_stem()?.to_string_lossy();
    let sdr = book.with_file_name(format!("{stem}.sdr"));
    let mut uuids: Vec<String> = std::fs::read_dir(sdr)
        .ok()?
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().to_string_lossy().into_owned();
            (e.path().is_dir() && is_uuid(&name)).then_some(name)
        })
        .collect();
    uuids.sort();
    uuids
        .into_iter()
        .map(|u| root.join("annotations").join(format!("{u}!!PDOC!!notebook.nbk")))
        .find(|p| p.is_file())
}

/// Which PDF page each annotation page belongs to. Pages without a placement
/// (the empty page the Kindle creates when the notebook is made) are skipped;
/// the rest are taken in reading order. See docs/format.md §9.
pub fn map_pages(pages: &[(Option<Placement>, usize)]) -> Vec<AnnotatedPage> {
    pages
        .iter()
        .enumerate()
        .filter_map(|(i, (p, strokes))| p.map(|p| (i, p, *strokes)))
        .enumerate()
        .filter(|(_, (_, _, strokes))| *strokes > 0)
        .map(|(pdf_page, (notebook_page, placement, strokes))| AnnotatedPage {
            pdf_page,
            notebook_page,
            placement,
            strokes,
        })
        .collect()
}

fn open(app: &App, id: &str) -> Result<Option<(PathBuf, Notebook)>> {
    let item = app.item(id)?;
    if item.kind != ItemKind::Book {
        return Ok(None);
    }
    let Some(path) = notebook_path(&app.layout.root, &app.item_path(&item)) else {
        return Ok(None);
    };
    let nb = Notebook::open(&path, &app.kdf_options(), &mut Diagnostics::default()).map_err(|e| e.to_string())?;
    Ok(Some((path, nb)))
}

/// The annotated pages of a book (empty when it has no handwriting).
pub fn pages(app: &App, id: &str) -> Result<Vec<AnnotatedPage>> {
    let Some((_, nb)) = open(app, id)? else {
        return Ok(Vec::new());
    };
    let mut diag = Diagnostics::default();
    let mut found = Vec::new();
    for i in 0..nb.page_count() {
        let p = nb.page(i, &mut diag).map_err(|e| e.to_string())?;
        found.push((p.placement, p.strokes.len() + p.graphics.len()));
    }
    Ok(map_pages(&found))
}

/// The handwriting of one annotation page as a transparent PNG covering the
/// page's canvas (its placement says where that sits on the PDF page).
pub fn overlay_png(app: &Arc<App>, id: &str, notebook_page: usize, width: u32) -> Result<Vec<u8>> {
    let Some((path, nb)) = open(app, id)? else {
        return Err(format!("{id} has no handwriting"));
    };
    let hash = blake3::hash(&std::fs::read(&path).map_err(|e| e.to_string())?).to_hex()[..32].to_string();
    let width = width_bucket(width);
    let (mut opts, variant) = app.reader_options();
    let key = app.cache.page_key(
        &hash,
        RENDERER_VERSION,
        &format!("{variant}-ink"),
        notebook_page as u32,
        width,
    );
    app.cached(&key, || {
        let mut diag = Diagnostics::default();
        if notebook_page >= nb.page_count() {
            return Err(format!("page {} of {}", notebook_page + 1, nb.page_count()));
        }
        let page = nb.page(notebook_page, &mut diag).map_err(|e| e.to_string())?;
        let (cw, ch) = page.canvas;
        opts.long_edge = ((f64::from(width) * cw.max(ch) as f64 / cw.max(1) as f64).round() as u32).max(1);
        Ok(render_page(&page, None, &opts).to_ink_png())
    })
}

/// The page's MediaBox `[x0 y0 x1 y1]`, inherited from parent nodes if needed.
fn media_box(doc: &lopdf::Document, page: lopdf::ObjectId) -> Option<[f64; 4]> {
    let mut node = doc.get_dictionary(page).ok()?;
    for _ in 0..32 {
        if let Ok(b) = node.get(b"MediaBox") {
            let b = match b {
                lopdf::Object::Reference(r) => doc.get_object(*r).ok()?,
                o => o,
            };
            let v: Vec<f64> = b
                .as_array()
                .ok()?
                .iter()
                .filter_map(|o| o.as_float().ok().map(f64::from))
                .collect();
            return (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]]);
        }
        node = doc
            .get_dictionary(node.get(b"Parent").ok()?.as_reference().ok()?)
            .ok()?;
    }
    None
}

/// Writes a copy of the book's PDF with the handwriting drawn on its pages as
/// vector paths (pencil as solid grey, highlighter at 20% opacity). The
/// original page content is kept as it is, so text stays selectable.
/// Returns the number of pages annotated.
pub fn export_pdf(app: &App, id: &str, dest: &Path) -> Result<usize> {
    let item = app.item(id)?;
    let Some((_, nb)) = open(app, id)? else {
        return Err(format!("{} has no handwriting", item.title));
    };
    let pages = pages(app, id)?;
    let mut doc = lopdf::Document::load(app.item_path(&item)).map_err(|e| e.to_string())?;
    let pdf_pages = doc.get_pages();
    let shapes = app.setting("draw_shapes") != "false";
    let mut gs: Option<lopdf::ObjectId> = None;
    let mut done = 0;
    for a in &pages {
        let Some(&page_id) = pdf_pages.get(&(a.pdf_page as u32 + 1)) else {
            continue;
        };
        let Some([x0, y0, x1, y1]) = media_box(&doc, page_id) else {
            continue;
        };
        let (w, h) = (x1 - x0, y1 - y0);
        let page = nb
            .page(a.notebook_page, &mut Diagnostics::default())
            .map_err(|e| e.to_string())?;
        let vector = folio_render::vector_page(&page, None, shapes);
        let (cw, ch) = (page.canvas.0.max(1) as f64, page.canvas.1.max(1) as f64);
        let p = a.placement;
        // canvas units (y down) → PDF points (y up) inside the canvas box
        let (sx, sy) = (p.width * w / cw, p.height * h / ch);
        let (tx, ty) = (x0 + p.left * w, y1 - p.top * h);
        let mut content = format!("Q q {sx:.6} 0 0 {:.6} {tx:.4} {ty:.4} cm\n", -sy).into_bytes();
        let mut body = vector.content.clone();
        if let Some(hl) = &vector.highlighter {
            // our writer draws highlighters as a group (/GsHl, /Hl); here they
            // are drawn in place at 20% opacity under a state of our own
            let gs_id = *gs.get_or_insert_with(|| {
                let mut d = lopdf::Dictionary::new();
                d.set("Type", "ExtGState");
                d.set("ca", 0.2);
                d.set("CA", 0.2);
                doc.add_object(d)
            });
            doc.add_graphics_state(page_id, "ScribeHl", gs_id)
                .map_err(|e| e.to_string())?;
            let call = format!(
                "q /{} gs /{} Do Q",
                folio_render::vector::HIGHLIGHTER_GSTATE,
                folio_render::vector::HIGHLIGHTER_XOBJECT
            );
            let mut inline = b"q /ScribeHl gs\n".to_vec();
            inline.extend_from_slice(hl);
            inline.extend_from_slice(b"Q");
            if let Some(at) = body.windows(call.len()).position(|w| w == call.as_bytes()) {
                body.splice(at..at + call.len(), inline);
            }
        }
        content.extend_from_slice(&body);
        content.extend_from_slice(b"Q\n");
        // the page's own content runs inside q … Q, so its state can't move the ink
        let first = doc.add_object(lopdf::Stream::new(lopdf::Dictionary::new(), b"q\n".to_vec()));
        let page_dict = doc.get_dictionary_mut(page_id).map_err(|e| e.to_string())?;
        let mut list = match page_dict.get(b"Contents") {
            Ok(lopdf::Object::Reference(r)) => vec![lopdf::Object::Reference(*r)],
            Ok(lopdf::Object::Array(v)) => v.clone(),
            _ => Vec::new(),
        };
        list.insert(0, lopdf::Object::Reference(first));
        page_dict.set("Contents", list);
        let mut stream = lopdf::Stream::new(lopdf::Dictionary::new(), content);
        let _ = stream.compress();
        let ink = doc.add_object(stream);
        if let Ok(lopdf::Object::Array(v)) = doc
            .get_dictionary_mut(page_id)
            .map_err(|e| e.to_string())?
            .get_mut(b"Contents")
        {
            v.push(lopdf::Object::Reference(ink));
        }
        done += 1;
    }
    doc.save(dest).map_err(|e| e.to_string())?;
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pl() -> Option<Placement> {
        Some(Placement {
            left: -0.03125,
            top: 0.0,
            width: 1.0625,
            height: 1.0,
        })
    }

    #[test]
    fn maps_placed_pages_in_order() {
        // a real sample: an empty unplaced page, then pages 1–3 written in order
        let m = map_pages(&[(None, 0), (pl(), 12), (pl(), 5), (pl(), 28)]);
        let got: Vec<(usize, usize)> = m.iter().map(|p| (p.pdf_page, p.notebook_page)).collect();
        assert_eq!(got, [(0, 1), (1, 2), (2, 3)]);
    }

    #[test]
    fn uuid_folders() {
        assert!(is_uuid("0356e7a2-d7bf-467b-b268-73a4952af05e"));
        assert!(!is_uuid("kpp_0356e7a2-d7bf-467b-b268-73a4952af05e"));
    }
}
