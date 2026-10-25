//! Notebook export: pages are decoded and rendered in parallel and streamed,
//! in order, into one PDF.
//!
//! Workers each own a [`Notebook`] handle (their own SQLite connection) and take
//! page numbers from a shared counter, never more than `window` pages ahead of
//! the writer, which bounds memory. The writer (the calling thread) reorders
//! pages and writes each as soon as it is next. Output goes to `<out>.part` and
//! is renamed into place when complete.

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use folio_kdf::OpenOptions;
use folio_kdf::ion::Diagnostics;
use folio_kdf::ion::diag::Entry;
use folio_nmdl::Notebook;
use folio_pdfout::{EncodedPage, PdfWriter, TemplateRef, encode_raster, encode_vector, page_size_pt};
use folio_render::geom::output_size;
use folio_render::{GrayImage, RasterOptions, render_page, render_template, template_content, vector_page};

use crate::{Error, Result};

#[derive(Clone, Copy, Debug)]
pub enum ExportMode {
    Raster(RasterOptions),
    /// Strokes as paths; pressure-shaded strokes in grey ("vector gray").
    Vector {
        shapes: bool,
    },
}

#[derive(Clone, Debug)]
pub struct ExportOptions {
    pub mode: ExportMode,
    /// Worker threads; 0 = one per CPU.
    pub jobs: usize,
    /// 0-based page numbers to export, in order; `None` = all.
    pub pages: Option<Vec<usize>>,
    pub cache_dir: Option<PathBuf>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            mode: ExportMode::Vector { shapes: true },
            jobs: 0,
            pages: None,
            cache_dir: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExportStats {
    pub pages: usize,
    pub bytes: u64,
    pub elapsed: Duration,
    pub diagnostics: Vec<Entry>,
}

struct Window {
    next: usize,
    written: usize,
    failed: bool,
}

struct Done {
    slot: usize,
    page: std::result::Result<EncodedPage, String>,
    template: Option<String>,
}

pub fn default_jobs() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get())
}

/// Exports a notebook to `out`. `progress(done, total)` is called after each
/// page is written; setting `cancel` stops the export and removes the partial file.
pub fn export_pdf(
    src: &Path,
    out: &Path,
    opts: &ExportOptions,
    cancel: Option<&AtomicBool>,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<ExportStats> {
    let start = Instant::now();
    let mut diag = Diagnostics::default();
    let notebook = Notebook::open(
        src,
        &OpenOptions {
            cache_dir: opts.cache_dir.clone(),
        },
        &mut diag,
    )?;
    let pages: Vec<usize> = match &opts.pages {
        Some(p) => p.iter().copied().filter(|&i| i < notebook.page_count()).collect(),
        None => (0..notebook.page_count()).collect(),
    };
    if pages.is_empty() {
        return Err(Error::Invalid("no pages to export".into()));
    }
    let jobs = if opts.jobs == 0 { default_jobs() } else { opts.jobs }
        .min(pages.len())
        .max(1);
    let window = 2 * jobs;

    let part = out.with_extension("pdf.part");
    let mut writer = PdfWriter::new(BufWriter::with_capacity(1 << 20, File::create(&part)?))?;
    let state = Mutex::new(Window {
        next: 0,
        written: 0,
        failed: false,
    });
    let wake = Condvar::new();
    let (tx, rx) = mpsc::channel::<Done>();
    let worker_diags = Mutex::new(Diagnostics::default());

    let result: Result<()> = std::thread::scope(|scope| {
        for _ in 0..jobs {
            let nb = notebook.try_clone()?;
            let (tx, state, wake, pages, worker_diags) = (tx.clone(), &state, &wake, &pages, &worker_diags);
            let mode = opts.mode;
            scope.spawn(move || {
                let mut diag = Diagnostics::default();
                let mut templates: HashMap<String, Option<GrayImage>> = HashMap::new();
                loop {
                    let slot = {
                        let mut st = state.lock().unwrap();
                        while !st.failed && st.next < pages.len() && st.next >= st.written + window {
                            st = wake.wait(st).unwrap();
                        }
                        if st.failed || st.next >= pages.len() {
                            break;
                        }
                        st.next += 1;
                        st.next - 1
                    };
                    let done = render_one(&nb, pages[slot], mode, &mut templates, &mut diag);
                    let (page, template) = match done {
                        Ok((p, t)) => (Ok(p), t),
                        Err(e) => (Err(format!("page {}: {e}", pages[slot] + 1)), None),
                    };
                    if tx.send(Done { slot, page, template }).is_err() {
                        break;
                    }
                }
                worker_diags.lock().unwrap().merge(diag);
            });
        }
        drop(tx);

        // Writer: reorder and stream pages out.
        let mut pending: BTreeMap<usize, Done> = BTreeMap::new();
        let mut forms: HashMap<String, Option<TemplateRef>> = HashMap::new();
        let mut written = 0;
        let fail = |msg: String| {
            state.lock().unwrap().failed = true;
            wake.notify_all();
            Err(Error::Invalid(msg))
        };
        for done in rx.iter() {
            pending.insert(done.slot, done);
            while let Some(done) = pending.remove(&written) {
                if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    return fail("export cancelled".into());
                }
                let mut page = match done.page {
                    Ok(p) => p,
                    Err(e) => return fail(e),
                };
                if let (EncodedPage::Vector { template, .. }, Some(id)) = (&mut page, &done.template) {
                    let form = match forms.get(id) {
                        Some(f) => *f,
                        None => {
                            let f = match notebook.template(id, &mut diag)? {
                                Some(t) => Some(writer.add_template(&template_content(&t), t.width, t.height)?),
                                None => None,
                            };
                            forms.insert(id.clone(), f);
                            f
                        }
                    };
                    *template = form;
                }
                writer.add_page(&page)?;
                written += 1;
                {
                    let mut st = state.lock().unwrap();
                    st.written = written;
                }
                wake.notify_all();
                progress(written, pages.len());
            }
        }
        if written != pages.len() {
            return fail(format!("only {written} of {} pages were rendered", pages.len()));
        }
        Ok(())
    });

    if let Err(e) = result {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    let title = src.file_stem().map(|s| s.to_string_lossy().into_owned());
    writer
        .finish(title.as_deref())?
        .into_inner()
        .map_err(|e| e.into_error())?
        .sync_all()?;
    fs::rename(&part, out)?;

    diag.merge(worker_diags.into_inner().unwrap());
    Ok(ExportStats {
        pages: pages.len(),
        bytes: fs::metadata(out)?.len(),
        elapsed: start.elapsed(),
        diagnostics: diag.entries().into_iter().cloned().collect(),
    })
}

/// Decodes and encodes one page. Returns the page and the template id it uses
/// (vector pages; the writer adds the template form).
fn render_one(
    nb: &Notebook,
    index: usize,
    mode: ExportMode,
    templates: &mut HashMap<String, Option<GrayImage>>,
    diag: &mut Diagnostics,
) -> Result<(EncodedPage, Option<String>)> {
    let page = nb.page(index, diag)?;
    let template_id = page
        .template_id
        .clone()
        .filter(|id| nb.outline().templates().any(|t| &t.name == id));
    match mode {
        ExportMode::Raster(ro) => {
            let (w, h) = output_size(page.canvas, ro.long_edge);
            let layer = match &template_id {
                Some(id) => {
                    let key = format!("{id}@{w}x{h}");
                    if !templates.contains_key(&key) {
                        let t = nb.template(id, diag)?.map(|t| render_template(&t, w, h));
                        templates.insert(key.clone(), t);
                    }
                    templates[&key].as_ref()
                }
                None => None,
            };
            let img = render_page(&page, layer, &ro);
            Ok((encode_raster(&img, page_size_pt(page.canvas)), None))
        }
        ExportMode::Vector { shapes } => {
            let tsize = match &template_id {
                Some(id) => nb.template(id, diag)?.map(|t| (t.width, t.height)),
                None => None,
            };
            let v = vector_page(&page, tsize, shapes);
            Ok((
                encode_vector(&v, page.canvas, None),
                template_id.filter(|_| tsize.is_some()),
            ))
        }
    }
}

/// Renders one page (0-based) to a grey image.
pub fn render_page_image(src: &Path, index: usize, opts: &RasterOptions) -> Result<GrayImage> {
    let mut diag = Diagnostics::default();
    let nb = Notebook::open(src, &OpenOptions::default(), &mut diag)?;
    if index >= nb.page_count() {
        return Err(Error::Invalid(format!("page {} of {}", index + 1, nb.page_count())));
    }
    let page = nb.page(index, &mut diag)?;
    let (w, h) = output_size(page.canvas, opts.long_edge);
    let template = match page.template_id.as_deref() {
        Some(id) => nb.template(id, &mut diag)?.map(|t| render_template(&t, w, h)),
        None => None,
    };
    Ok(render_page(&page, template.as_ref(), opts))
}
