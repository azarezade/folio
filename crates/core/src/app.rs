//! The application state shared by the GUI and the dev bridge: library index,
//! render cache, open notebooks, background jobs with progress and cancel,
//! the transfer queue and the Kindle's status.
//!
//! Long work runs as jobs on their own threads and reports through the event
//! sink; nothing blocks the caller. Device work takes `device_lock`, so there is
//! one MTP session at a time and transfers run one after another.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use folio_device::{DeviceEvent, Error as DeviceError, Kindle};
use folio_kdf::OpenOptions;
use folio_kdf::ion::Diagnostics;
use folio_library::Layout;
use folio_library::cache::{Cache, width_bucket};
use folio_library::index::{Index, Item, ItemKind};
use folio_nmdl::Notebook;
use folio_render::geom::output_size;
use folio_render::{PencilMode, RENDERER_VERSION, RasterOptions, render_page, render_template};
use serde::Serialize;
use serde_json::{Value, json};

pub type EventSink = Arc<dyn Fn(&Event) + Send + Sync>;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Job(JobInfo),
    JobLog {
        id: u64,
        line: String,
    },
    Device(DeviceStatus),
    Transfer(Transfer),
    /// Items were added, removed or changed: refetch the lists.
    Library,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
pub struct JobInfo {
    pub id: u64,
    pub kind: String,
    pub title: String,
    pub state: JobState,
    pub done: u64,
    pub total: u64,
    pub message: String,
    pub result: Option<Value>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct DeviceStatus {
    pub connected: bool,
    pub name: Option<String>,
    /// macOS holds the Kindle (ptpcamerad); "Free the Kindle" can help.
    pub busy: bool,
    pub holder: Option<String>,
    pub free_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Down,
    Up,
}

#[derive(Clone, Debug, Serialize)]
pub struct Transfer {
    pub id: u64,
    pub job: u64,
    pub direction: Direction,
    pub name: String,
    pub size: u64,
    pub done: u64,
    pub state: JobState,
    pub bytes_per_sec: f64,
    pub error: Option<String>,
    /// What to run again on retry: a local path (uploads) or device path (downloads).
    #[serde(skip)]
    pub retry: Option<RetrySpec>,
}

#[derive(Clone, Debug)]
pub enum RetrySpec {
    Push { path: PathBuf, to: String },
    Pull { device_path: String },
}

/// Settings with their defaults (stored in the index's settings table).
pub const SETTINGS: &[(&str, &str)] = &[
    ("export_mode", "vector"),
    ("export_long_edge", "2480"),
    ("export_dir", ""),
    ("pencil_scale", "4"),
    ("reader_pencil", "texture"),
    ("draw_shapes", "true"),
    ("cache_cap_mb", "2048"),
    ("auto_sync", "false"),
    ("sync_db", "false"),
    ("library_view", "grid"),
    ("library_sort", "added"),
    ("theme", "system"),
    ("book_mode", "false"),
    ("last_item", ""),
    ("ptpcamerad_disabled", "false"),
];

struct JobEntry {
    info: JobInfo,
    cancel: Arc<AtomicBool>,
}

pub struct App {
    pub layout: Layout,
    pub cache: Cache,
    index: Mutex<Index>,
    /// A few open notebooks, most recent last.
    notebooks: Mutex<Vec<(String, Arc<Mutex<Notebook>>)>>,
    rendering: Mutex<HashSet<PathBuf>>,
    rendered: Condvar,
    jobs: Mutex<BTreeMap<u64, JobEntry>>,
    next_id: AtomicU64,
    pub(crate) device_lock: Mutex<()>,
    /// Exports run one at a time, each its own cancellable job.
    pub(crate) export_lock: Mutex<()>,
    device: Mutex<DeviceStatus>,
    transfers: Mutex<Vec<Transfer>>,
    cache_size: Mutex<Option<(Instant, u64)>>,
    puts: AtomicU64,
    /// A text-indexing job is running.
    text_indexing: AtomicBool,
    sink: EventSink,
}

const OPEN_NOTEBOOKS: usize = 3;

pub type Result<T> = std::result::Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub struct JobCtx {
    pub id: u64,
    pub app: Arc<App>,
    pub cancel: Arc<AtomicBool>,
}

impl JobCtx {
    pub fn progress(&self, done: u64, total: u64, message: impl Into<String>) {
        self.app.update_job(self.id, |j| {
            j.done = done;
            j.total = total;
            j.message = message.into();
        });
    }

    pub fn log(&self, line: impl Into<String>) {
        (self.app.sink)(&Event::JobLog {
            id: self.id,
            line: line.into(),
        });
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

impl App {
    pub fn new(layout: Layout, cache: Cache, sink: EventSink) -> Result<Arc<App>> {
        let index = Index::open(&layout).map_err(err)?;
        let app = Arc::new(App {
            layout,
            cache,
            index: Mutex::new(index),
            notebooks: Mutex::new(Vec::new()),
            rendering: Mutex::new(HashSet::new()),
            rendered: Condvar::new(),
            jobs: Mutex::new(BTreeMap::new()),
            next_id: AtomicU64::new(1),
            device_lock: Mutex::new(()),
            export_lock: Mutex::new(()),
            device: Mutex::new(DeviceStatus::default()),
            transfers: Mutex::new(Vec::new()),
            cache_size: Mutex::new(None),
            puts: AtomicU64::new(0),
            text_indexing: AtomicBool::new(false),
            sink,
        });
        app.scan()?;
        Ok(app)
    }

    pub fn emit(&self, e: Event) {
        (self.sink)(&e);
    }

    pub(crate) fn kdf_options(&self) -> OpenOptions {
        OpenOptions {
            cache_dir: Some(self.cache.root.join("kdf")),
        }
    }

    /// Re-reads the library folder into the index. Notebooks that changed are
    /// opened to count their pages.
    pub fn scan(self: &Arc<Self>) -> Result<usize> {
        let opts = self.kdf_options();
        let changed = self
            .index
            .lock()
            .unwrap()
            .scan(&|path: &Path| {
                Notebook::open(path, &opts, &mut Diagnostics::default())
                    .ok()
                    .map(|n| n.page_count() as u32)
            })
            .map_err(err)?;
        if changed > 0 {
            self.notebooks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
            self.emit(Event::Library);
        }
        self.index_text();
        Ok(changed)
    }

    /// Indexes the text of new or changed books for search, in a background
    /// job (one at a time; a scan during it is picked up by the next one).
    pub fn index_text(self: &Arc<Self>) {
        let todo = self.with_index(|i| i.unindexed_books()).unwrap_or_default();
        if todo.is_empty() || self.text_indexing.swap(true, Ordering::SeqCst) {
            return;
        }
        let app = self.clone();
        self.spawn_job("index", "Index books for search", move |ctx| {
            struct Done(Arc<App>);
            impl Drop for Done {
                fn drop(&mut self) {
                    self.0.text_indexing.store(false, Ordering::SeqCst);
                }
            }
            let _done = Done(app.clone());
            let total = todo.len() as u64;
            let mut chunks_total = 0;
            for (n, item) in todo.iter().enumerate() {
                if ctx.cancelled() {
                    break;
                }
                ctx.progress(n as u64, total, item.title.clone());
                let path = app.item_path(item);
                let chunks = match folio_kfx::text::extract(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        ctx.log(format!("{}: {e}", item.title));
                        Vec::new()
                    }
                };
                let rows: Vec<(String, String, String)> =
                    chunks.into_iter().map(|c| (c.loc, c.label, c.text)).collect();
                chunks_total += rows.len();
                app.index
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .set_text(&item.id, &item.content_hash, &rows)
                    .map_err(err)?;
            }
            ctx.progress(total, total, "done");
            Ok(json!({"books": total, "chunks": chunks_total}))
        });
    }

    pub fn with_index<T>(&self, f: impl FnOnce(&Index) -> folio_library::Result<T>) -> Result<T> {
        f(&self.index.lock().unwrap_or_else(std::sync::PoisonError::into_inner)).map_err(err)
    }

    pub fn item(&self, id: &str) -> Result<Item> {
        self.with_index(|i| i.get(id))?.ok_or_else(|| format!("no item {id}"))
    }

    pub fn item_path(&self, item: &Item) -> PathBuf {
        self.layout.root.join(&item.rel_path)
    }

    pub fn setting(&self, key: &str) -> String {
        let default = SETTINGS.iter().find(|(k, _)| *k == key).map_or("", |(_, v)| v);
        self.with_index(|i| i.setting(key))
            .ok()
            .flatten()
            .unwrap_or_else(|| default.to_string())
    }

    pub fn settings(&self) -> BTreeMap<String, String> {
        SETTINGS.iter().map(|(k, _)| (k.to_string(), self.setting(k))).collect()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        if !SETTINGS.iter().any(|(k, _)| *k == key) {
            return Err(format!("unknown setting {key}"));
        }
        self.with_index(|i| i.set_setting(key, value))
    }

    /// An open handle on a notebook (kept for the next page request).
    pub fn notebook(&self, item: &Item) -> Result<Arc<Mutex<Notebook>>> {
        let mut open = self.notebooks.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(pos) = open.iter().position(|(h, _)| *h == item.content_hash) {
            let entry = open.remove(pos);
            let nb = entry.1.clone();
            open.push(entry);
            return Ok(nb);
        }
        let nb = Notebook::open(self.item_path(item), &self.kdf_options(), &mut Diagnostics::default()).map_err(err)?;
        let nb = Arc::new(Mutex::new(nb));
        open.push((item.content_hash.clone(), nb.clone()));
        if open.len() > OPEN_NOTEBOOKS {
            open.remove(0);
        }
        Ok(nb)
    }

    pub(crate) fn reader_options(&self) -> (RasterOptions, String) {
        let scale = self.setting("pencil_scale").parse().unwrap_or(4);
        let gray = self.setting("reader_pencil") == "gray";
        let shapes = self.setting("draw_shapes") != "false";
        let pencil = if gray {
            PencilMode::Gray
        } else {
            PencilMode::Texture { scale }
        };
        let variant = format!(
            "{}-{}",
            if gray {
                "gray".to_string()
            } else {
                format!("tex{scale}")
            },
            if shapes { "s" } else { "n" }
        );
        (
            RasterOptions {
                long_edge: 2480,
                pencil,
                shapes,
            },
            variant,
        )
    }

    /// A notebook page as PNG, rendered at `width` rounded up to a 256-px bucket
    /// and cached by content hash, renderer version and options.
    pub fn page_png(&self, id: &str, page: u32, width: u32) -> Result<Vec<u8>> {
        let item = self.item(id)?;
        if item.kind != ItemKind::Notebook {
            return Err(format!("{id} is not a notebook"));
        }
        let width = width_bucket(width);
        let (mut opts, variant) = self.reader_options();
        let key = self
            .cache
            .page_key(&item.content_hash, RENDERER_VERSION, &variant, page, width);
        self.cached(&key, || {
            let nb = self.notebook(&item)?;
            let (page_data, template) = {
                let nb = nb.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if page as usize >= nb.page_count() {
                    return Err(format!("page {} of {}", page + 1, nb.page_count()));
                }
                let mut diag = Diagnostics::default();
                let p = nb.page(page as usize, &mut diag).map_err(err)?;
                let t = match p.template_id.as_deref() {
                    Some(t) => nb.template(t, &mut diag).map_err(err)?,
                    None => None,
                };
                (p, t)
            };
            let (cw, ch) = page_data.canvas;
            opts.long_edge = ((f64::from(width) * cw.max(ch) as f64 / cw.max(1) as f64).round() as u32).max(1);
            let (w, h) = output_size(page_data.canvas, opts.long_edge);
            let layer = template.map(|t| render_template(&t, w, h));
            Ok(render_page(&page_data, layer.as_ref(), &opts).to_png())
        })
    }

    /// The device thumbnail if there is one, else page 1 at 256 px.
    /// A thumbnail: PNG for notebooks and screenshots, the cover image (as
    /// stored in the book) for books.
    pub fn thumbnail(&self, id: &str) -> Result<(&'static str, Vec<u8>)> {
        let item = self.item(id)?;
        if item.kind == ItemKind::Book {
            let c = folio_kfx::cover(&self.item_path(&item)).ok_or("no cover")?;
            return Ok((c.mime, c.data));
        }
        Ok(("image/png", self.thumbnail_png(&item)?))
    }

    fn thumbnail_png(&self, item: &folio_library::index::Item) -> Result<Vec<u8>> {
        let id = item.id.as_str();
        match item.kind {
            ItemKind::Notebook => {
                if let Some(t) = &item.thumbnail
                    && let Ok(data) = std::fs::read(self.layout.root.join(t))
                {
                    return Ok(data);
                }
                self.page_png(id, 0, 256)
            }
            ItemKind::Screenshot => {
                let key = self.cache.thumb_key(&item.content_hash, RENDERER_VERSION);
                let path = self.item_path(item);
                self.cached(&key, || {
                    crate::image::downscale_png(&std::fs::read(&path).map_err(err)?, 256)
                })
            }
            ItemKind::Book => Err("books use their cover".into()),
        }
    }

    /// Returns the cached bytes for `key`, or makes them once (concurrent
    /// requests for the same key wait for the first).
    pub(crate) fn cached(&self, key: &Path, make: impl FnOnce() -> Result<Vec<u8>>) -> Result<Vec<u8>> {
        if let Some(data) = self.cache.get(key) {
            return Ok(data);
        }
        {
            let mut busy = self.rendering.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            while busy.contains(key) {
                busy = self
                    .rendered
                    .wait(busy)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            if let Some(data) = self.cache.get(key) {
                return Ok(data);
            }
            busy.insert(key.to_path_buf());
        }
        let result = make();
        if let Ok(data) = &result {
            let _ = self.cache.put(key, data);
            if self.puts.fetch_add(1, Ordering::Relaxed) % 200 == 199 {
                self.prune_cache();
            }
        }
        self.rendering
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(key);
        self.rendered.notify_all();
        result
    }

    pub fn prune_cache(&self) {
        let cap = self.setting("cache_cap_mb").parse::<u64>().unwrap_or(2048) * 1_000_000;
        self.cache.prune(cap);
        *self
            .cache_size
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    /// Cache size, recomputed at most every 30 s.
    pub fn cache_bytes(&self) -> u64 {
        let mut c = self
            .cache_size
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match *c {
            Some((t, n)) if t.elapsed() < Duration::from_secs(30) => n,
            _ => {
                let n = self.cache.size();
                *c = Some((Instant::now(), n));
                n
            }
        }
    }

    pub fn clear_cache(&self) -> Result<u64> {
        self.notebooks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        let n = self.cache.clear().map_err(err)?;
        *self
            .cache_size
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(n)
    }

    // ── jobs ────────────────────────────────────────────────────────────────

    pub fn jobs(&self) -> Vec<JobInfo> {
        self.jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .map(|j| j.info.clone())
            .collect()
    }

    fn update_job(&self, id: u64, f: impl FnOnce(&mut JobInfo)) {
        let info = {
            let mut jobs = self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(j) = jobs.get_mut(&id) else { return };
            f(&mut j.info);
            j.info.clone()
        };
        self.emit(Event::Job(info));
    }

    pub fn cancel_job(&self, id: u64) -> bool {
        match self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&id)
        {
            Some(j) => {
                j.cancel.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// Runs `work` on its own thread as a job. Finished jobs are kept (the last 50).
    pub fn spawn_job(
        self: &Arc<Self>,
        kind: &str,
        title: impl Into<String>,
        work: impl FnOnce(&JobCtx) -> Result<Value> + Send + 'static,
    ) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        let info = JobInfo {
            id,
            kind: kind.into(),
            title: title.into(),
            state: JobState::Queued,
            done: 0,
            total: 0,
            message: String::new(),
            result: None,
        };
        {
            let mut jobs = self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            jobs.insert(
                id,
                JobEntry {
                    info: info.clone(),
                    cancel: cancel.clone(),
                },
            );
            while jobs.len() > 50 {
                let Some(old) = jobs
                    .iter()
                    .find(|(_, j)| !matches!(j.info.state, JobState::Queued | JobState::Running))
                    .map(|(k, _)| *k)
                else {
                    break;
                };
                jobs.remove(&old);
            }
        }
        self.emit(Event::Job(info));
        let ctx = JobCtx {
            id,
            app: self.clone(),
            cancel,
        };
        std::thread::Builder::new()
            .name(format!("job-{kind}-{id}"))
            .spawn(move || {
                ctx.app.update_job(id, |j| j.state = JobState::Running);
                // a panic must still end the job, or it would show as running forever
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&ctx))).unwrap_or_else(|p| {
                        let msg = p
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                            .unwrap_or_else(|| "unknown error".into());
                        Err(format!("internal error: {msg}"))
                    });
                let cancelled = ctx.cancelled();
                ctx.app.update_job(id, |j| match result {
                    Ok(v) => {
                        j.state = if cancelled { JobState::Cancelled } else { JobState::Done };
                        j.result = Some(v);
                    }
                    Err(e) => {
                        j.state = if cancelled {
                            JobState::Cancelled
                        } else {
                            JobState::Failed
                        };
                        j.message = e;
                    }
                });
            })
            .expect("spawn job thread");
        id
    }

    // ── transfers ───────────────────────────────────────────────────────────

    pub fn transfers(&self) -> Vec<Transfer> {
        self.transfers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn add_transfer(
        &self,
        job: u64,
        direction: Direction,
        name: &str,
        size: u64,
        retry: Option<RetrySpec>,
    ) -> u64 {
        let t = Transfer {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            job,
            direction,
            name: name.into(),
            size,
            done: 0,
            state: JobState::Queued,
            bytes_per_sec: 0.0,
            error: None,
            retry,
        };
        let id = t.id;
        {
            let mut ts = self.transfers.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            ts.push(t.clone());
            let finished = ts.iter().filter(|t| matches!(t.state, JobState::Done)).count();
            if finished > 200
                && let Some(pos) = ts.iter().position(|t| t.state == JobState::Done)
            {
                ts.remove(pos);
            }
        }
        self.emit(Event::Transfer(t));
        id
    }

    pub(crate) fn update_transfer(&self, id: u64, f: impl FnOnce(&mut Transfer)) {
        let t = {
            let mut ts = self.transfers.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(t) = ts.iter_mut().find(|t| t.id == id) else {
                return;
            };
            f(t);
            t.clone()
        };
        self.emit(Event::Transfer(t));
    }

    pub fn clear_finished_transfers(&self) {
        self.transfers
            .lock()
            .unwrap()
            .retain(|t| matches!(t.state, JobState::Queued | JobState::Running));
    }

    pub(crate) fn transfer_retry(&self, id: u64) -> Option<RetrySpec> {
        self.transfers
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.retry.clone())
    }

    // ── device ──────────────────────────────────────────────────────────────

    pub fn device_status(&self) -> DeviceStatus {
        self.device
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn set_device(&self, s: DeviceStatus) {
        *self.device.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = s.clone();
        self.emit(Event::Device(s));
    }

    /// Opens the Kindle under the device lock, updating the status either way.
    pub fn with_kindle<T>(&self, f: impl FnOnce(&mut Kindle) -> Result<T>) -> Result<T> {
        let _session = self
            .device_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match Kindle::open() {
            Ok(mut k) => {
                self.set_device(status_of(&k));
                let r = f(&mut k);
                if let Ok(info) = k.refresh() {
                    self.set_device(DeviceStatus {
                        free_bytes: Some(info.free_bytes),
                        total_bytes: Some(info.total_bytes),
                        ..self.device_status()
                    });
                }
                let _ = k.close();
                r
            }
            Err(e) => {
                self.set_device(failed_status(&e));
                Err(device_error(&e))
            }
        }
    }

    /// Looks at the Kindle now (connect, read storage, disconnect).
    pub fn refresh_device(&self) -> DeviceStatus {
        if folio_device::list_kindles().map(|k| k.is_empty()).unwrap_or(true) {
            self.set_device(DeviceStatus::default());
            return self.device_status();
        }
        if self.device_lock.try_lock().is_err() {
            return self.device_status(); // a job is using it; the status is current
        }
        let _ = self.with_kindle(|_| Ok(()));
        self.device_status()
    }

    /// Follows plug and unplug events on a background thread.
    pub fn start_device_watch(self: &Arc<Self>) {
        let app = self.clone();
        std::thread::Builder::new()
            .name("device-watch".into())
            .spawn(move || {
                let _ = folio_device::watch(|e| {
                    match e {
                        DeviceEvent::Connected(_) => {
                            std::thread::sleep(Duration::from_millis(500));
                            let s = app.refresh_device();
                            if s.connected && app.setting("auto_sync") == "true" {
                                crate::ops::start_sync(&app, folio_library::sync::SyncOptions::all());
                            }
                        }
                        DeviceEvent::Disconnected(_) => app.set_device(DeviceStatus::default()),
                    }
                    true
                });
            })
            .expect("spawn device watch");
    }
}

pub(crate) fn status_of(k: &Kindle) -> DeviceStatus {
    let i = k.info();
    DeviceStatus {
        connected: true,
        name: Some(i.name.clone()),
        busy: false,
        holder: None,
        free_bytes: Some(i.free_bytes),
        total_bytes: Some(i.total_bytes),
        error: None,
    }
}

fn failed_status(e: &DeviceError) -> DeviceStatus {
    match e {
        DeviceError::NotConnected => DeviceStatus::default(),
        DeviceError::Busy { holder } => DeviceStatus {
            connected: true,
            name: Some("Kindle".into()),
            busy: true,
            holder: holder.clone(),
            ..DeviceStatus::default()
        },
        other => DeviceStatus {
            connected: true,
            error: Some(other.to_string()),
            ..DeviceStatus::default()
        },
    }
}

pub(crate) fn device_error(e: &DeviceError) -> String {
    match e {
        DeviceError::Busy { .. } => format!("{e}. Use “Free the Kindle” to ask macOS to let go of it."),
        DeviceError::NotConnected => "No Kindle is connected. Plug it in and unlock it.".into(),
        other => other.to_string(),
    }
}

/// The counts the status bar and tabs show.
pub fn counts(app: &App) -> HashMap<String, usize> {
    app.with_index(|i| i.counts())
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v)| (format!("{k:?}").to_lowercase(), v))
        .collect()
}
