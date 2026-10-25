//! Job system, progress events and the public API used by the UI and CLI.
//!
//! [`app::App`] holds the library, cache, jobs and device state; [`api`] is the
//! single entry point for the UI (Tauri and the dev bridge); [`export`] is the
//! parallel PDF exporter; [`inspect`] backs `folio-cli inspect`.

pub mod annotations;
pub mod api;
pub mod app;
pub mod export;
mod image;
pub mod ops;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use folio_kdf::ion::Diagnostics;
use folio_kdf::ion::diag::Entry;
use folio_kdf::{Kdf, OpenOptions, usage};
use folio_nmdl::{Notebook, ReadingOrder, SectionKind};
use serde::Serialize;

pub use folio_kdf::{Error, Result};

#[derive(Clone, Debug, Default, Serialize)]
pub struct FragmentCounts {
    pub total: usize,
    pub by_type: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TemplateInfo {
    pub section: String,
    pub template_type: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct InspectReport {
    pub path: PathBuf,
    pub size: u64,
    pub fingerprints_removed: usize,
    pub is_notebook: bool,
    /// Every fragment in the database (after the delta overlay), plus the ones
    /// kfxlib synthesises (`$270`, `$593`).
    pub stored: FragmentCounts,
    /// The fragments kfxlib keeps after its reachability check.
    pub reachable: FragmentCounts,
    pub missing_references: usize,
    pub unreferenced: usize,
    pub undefined_eids: usize,
    pub undefined_eid_sample: Vec<String>,
    pub reading_orders: Vec<ReadingOrder>,
    pub sections_processed: usize,
    pub sections_by_kind: BTreeMap<String, usize>,
    pub page_count: usize,
    pub page_sections: Vec<String>,
    pub canvas_sizes: BTreeMap<String, usize>,
    pub templates: Vec<TemplateInfo>,
    pub unreferenced_pages: Vec<String>,
    /// Filled when pages are decoded (`inspect(…, decode_pages: true)`).
    pub strokes: Option<StrokeSummary>,
    pub diagnostics: Vec<Entry>,
    pub timing_ms: BTreeMap<&'static str, u128>,
}

fn counts<'a>(kdf: &Kdf, frags: impl Iterator<Item = &'a folio_kdf::Fragment>) -> FragmentCounts {
    let mut c = FragmentCounts::default();
    for f in frags {
        c.total += 1;
        *c.by_type.entry(kdf.symtab().name(f.ftype()).into_owned()).or_default() += 1;
    }
    c
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct StrokeSummary {
    pub strokes: usize,
    pub points: usize,
    pub stroke_groups: usize,
    pub brushes: BTreeMap<String, usize>,
    pub transformed: usize,
    /// `$272` vector graphics (kfxlib skips them), and their shapes by kind.
    pub graphics: usize,
    pub shapes: BTreeMap<String, usize>,
    /// Other content kfxlib skips, by content type.
    pub unhandled_content: BTreeMap<String, usize>,
    /// The first unhandled node, as Ion text.
    pub unhandled_example: Option<String>,
}

/// Opens a notebook (or any KDF) and reports its fragments, sections and pages.
/// With `decode_pages`, every page is decoded too and strokes are summarised.
pub fn inspect(path: &Path, cache_dir: Option<PathBuf>, decode_pages: bool) -> Result<InspectReport> {
    let mut timing = BTreeMap::new();
    let t = Instant::now();
    let kdf = Kdf::open(path, &OpenOptions { cache_dir })?;
    timing.insert("open", t.elapsed().as_millis());

    let mut diag = Diagnostics::default();
    diag.merge(kdf.diagnostics().clone());

    let t = Instant::now();
    let u = usage::analyze(&kdf, &mut diag)?;
    timing.insert("reachability", t.elapsed().as_millis());

    let t = Instant::now();
    let outline = folio_nmdl::outline(&kdf, &mut diag)?;
    timing.insert("outline", t.elapsed().as_millis());

    let mut strokes = None;
    let kdf = if decode_pages {
        let t = Instant::now();
        let notebook = Notebook::from_kdf(kdf, &mut Diagnostics::default())?;
        let mut sum = StrokeSummary::default();
        for i in 0..notebook.page_count() {
            let page = notebook.page(i, &mut diag)?;
            sum.stroke_groups += page.stroke_groups;
            for s in &page.strokes {
                sum.strokes += 1;
                sum.points += s.points.len();
                sum.transformed += s.transform.is_some() as usize;
                *sum.brushes.entry(s.brush.name()).or_default() += 1;
            }
            for g in &page.graphics {
                sum.graphics += 1;
                for sh in &g.shapes {
                    *sum.shapes.entry(sh.kind.clone()).or_default() += 1;
                }
            }
            for u in &page.unhandled {
                *sum.unhandled_content.entry(u.content_type.clone()).or_default() += 1;
                if sum.unhandled_example.is_none() {
                    sum.unhandled_example = Some(format!("page {}: {}", page.index, u.text));
                }
            }
        }
        timing.insert("decode_pages", t.elapsed().as_millis());
        strokes = Some(sum);
        notebook.into_kdf()
    } else {
        kdf
    };

    let mut sections_by_kind = BTreeMap::new();
    let mut canvas_sizes = BTreeMap::new();
    for s in &outline.sections {
        let kind = match s.kind {
            SectionKind::Page => "page",
            SectionKind::Template => "template",
            SectionKind::Other => "other",
        };
        *sections_by_kind.entry(kind.to_string()).or_default() += 1;
        if let Some((w, h)) = s.canvas {
            *canvas_sizes.entry(format!("{w}x{h}")).or_default() += 1;
        }
    }

    Ok(InspectReport {
        path: path.to_path_buf(),
        size: std::fs::metadata(path)?.len(),
        fingerprints_removed: kdf.fingerprints_removed(),
        is_notebook: kdf.is_notebook(),
        stored: counts(&kdf, kdf.fragments().iter()),
        reachable: counts(&kdf, u.reachable.iter().map(|&i| &kdf.fragments()[i])),
        missing_references: u.missing.len(),
        unreferenced: u.unreferenced.len(),
        undefined_eids: u.undefined_eids,
        undefined_eid_sample: u.undefined_eid_sample.clone(),
        sections_processed: outline.sections.len(),
        sections_by_kind,
        page_count: outline.pages().count(),
        page_sections: outline.pages().map(|s| s.name.clone()).collect(),
        canvas_sizes,
        templates: outline
            .templates()
            .map(|s| TemplateInfo {
                section: s.name.clone(),
                template_type: s.template_type.clone(),
            })
            .collect(),
        unreferenced_pages: outline.unreferenced_pages.clone(),
        strokes,
        reading_orders: outline.reading_orders,
        diagnostics: diag.entries().into_iter().cloned().collect(),
        timing_ms: timing,
    })
}
