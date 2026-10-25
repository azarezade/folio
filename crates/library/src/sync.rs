//! The sync engine: one MTP session, one walk of the device, then
//! copies of what is new or changed.
//!
//! Change detection: the Kindle reports no dates over MTP, so a size match is
//! not enough to call a file unchanged. Notebooks (and the sync database) are
//! SQLite: their header's file change counter (bytes 24–27) is read from the
//! device with a 100-byte partial read and compared with the local copy's.
//! Other files compare by size.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use folio_device::{Entry, Error as DeviceError, Handle, Kindle};
use serde::{Deserialize, Serialize};

use crate::{Error, Layout, Result, classify_book, safe_component};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Notebook,
    Thumbnail,
    /// Handwriting on a book or PDF (`.notebooks/<id>!!PDOC!!notebook/nbk`).
    Annotation,
    Book,
    /// Files in a book's `.sdr` folder (reading position, annotations).
    BookSidecar,
    Screenshot,
    /// `.sync/KSDKNoteSyncDB.sqlite` (notebook titles and folders; Phase 8).
    SyncDb,
}

impl Kind {
    pub fn category(self) -> Category {
        match self {
            Kind::Notebook | Kind::Thumbnail | Kind::Annotation | Kind::SyncDb => Category::Notebooks,
            Kind::Book | Kind::BookSidecar => Category::Books,
            Kind::Screenshot => Category::Screenshots,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Notebooks,
    Books,
    Screenshots,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SyncOptions {
    pub notebooks: bool,
    pub books: bool,
    pub screenshots: bool,
    /// Copy everything, even what looks up to date.
    pub force: bool,
    /// Also copy `.sync/KSDKNoteSyncDB.sqlite`. Off by default: it is ~900 MB
    /// on a well-used Scribe and changes with every edit; it holds no notebook titles.
    pub sync_db: bool,
}

impl SyncOptions {
    pub fn all() -> SyncOptions {
        SyncOptions {
            notebooks: true,
            books: true,
            screenshots: true,
            force: false,
            sync_db: false,
        }
    }

    fn wants(&self, c: Category) -> bool {
        match c {
            Category::Notebooks => self.notebooks,
            Category::Books => self.books,
            Category::Screenshots => self.screenshots,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    New,
    Changed,
    UpToDate,
}

#[derive(Clone, Debug, Serialize)]
pub struct SyncItem {
    pub kind: Kind,
    pub device_path: String,
    #[serde(skip)]
    pub handle: Handle,
    pub size: u64,
    /// SQLite file change counter on the device (notebooks, sync db).
    pub counter: Option<u32>,
    /// Path relative to the library root.
    pub local: PathBuf,
    pub action: Action,
    /// Why it is changed, for the log.
    pub reason: Option<String>,
    /// The same file is already in the library at this (older, flattened)
    /// path, as the Python app saved it: move it instead of downloading.
    pub relocate_from: Option<PathBuf>,
}

/// What a sync would do, computed from the device listing before anything is copied.
#[derive(Clone, Debug, Default, Serialize)]
pub struct SyncPlan {
    pub items: Vec<SyncItem>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CategorySummary {
    pub found: usize,
    pub new: usize,
    pub changed: usize,
    pub up_to_date: usize,
    pub bytes_to_copy: u64,
}

impl SyncPlan {
    pub fn to_copy(&self) -> impl Iterator<Item = &SyncItem> {
        self.items.iter().filter(|i| i.action != Action::UpToDate)
    }

    pub fn summary(&self) -> BTreeMap<Category, CategorySummary> {
        let mut out: BTreeMap<Category, CategorySummary> = BTreeMap::new();
        for i in &self.items {
            // thumbnails, sidecars and the sync db are counted in bytes only
            let primary = matches!(
                i.kind,
                Kind::Notebook | Kind::Annotation | Kind::Book | Kind::Screenshot
            );
            let s = out.entry(i.kind.category()).or_default();
            if primary {
                s.found += 1;
                match i.action {
                    Action::New => s.new += 1,
                    Action::Changed => s.changed += 1,
                    Action::UpToDate => s.up_to_date += 1,
                }
            }
            if i.action != Action::UpToDate && i.relocate_from.is_none() {
                s.bytes_to_copy += i.size;
            }
        }
        out
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SyncReport {
    pub device: String,
    pub found: BTreeMap<Category, usize>,
    pub copied: BTreeMap<Category, usize>,
    pub skipped: BTreeMap<Category, usize>,
    pub bytes_copied: u64,
    pub errors: Vec<String>,
    pub log: Vec<String>,
    pub seconds: f64,
}

/// What was copied from where, kept in the library as `.folio-sync.json`, so
/// a delete can find the device file and check it is the same one.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub files: BTreeMap<String, ManifestEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub kind: Kind,
    pub device_path: String,
    pub size: u64,
    pub counter: Option<u32>,
    /// Unix seconds.
    pub synced_at: u64,
}

impl Manifest {
    pub fn load(layout: &Layout) -> Manifest {
        std::fs::read_to_string(layout.manifest())
            .or_else(|_| std::fs::read_to_string(layout.legacy_manifest()))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, layout: &Layout) -> Result<()> {
        let path = layout.manifest();
        let tmp = path.with_extension("json.part");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(self).map_err(|e| Error::Other(e.to_string()))?,
        )?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

const UUID_LEN: usize = 36;

fn is_uuid(s: &str) -> bool {
    s.len() == UUID_LEN
        && s.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

const BOOK_EXTENSIONS: [&str; 11] = [
    "pdf", "epub", "kfx", "azw", "azw3", "azw8", "mobi", "prc", "txt", "kfx-zip", "docx",
];
const IMAGE_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

fn ext(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// SQLite's file change counter, from the first 28+ bytes of a database.
pub fn sqlite_counter(head: &[u8]) -> Option<u32> {
    (head.len() >= 28 && head.starts_with(b"SQLite format 3\0"))
        .then(|| u32::from_be_bytes(head[24..28].try_into().unwrap()))
}

fn local_counter(path: &Path) -> Option<u32> {
    use std::io::Read;
    let mut head = [0u8; 28];
    std::fs::File::open(path).ok()?.read_exact(&mut head).ok()?;
    sqlite_counter(&head)
}

/// Walks the device once and decides, per file, whether it needs copying.
pub fn plan(kindle: &Kindle, layout: &Layout, opts: &SyncOptions, log: &mut dyn FnMut(String)) -> Result<SyncPlan> {
    let mut found: Vec<(Kind, String, Entry, PathBuf)> = Vec::new();
    let root = kindle.list(None)?;
    let folder = |name: &str| {
        root.iter()
            .find(|e| e.is_folder && e.name.eq_ignore_ascii_case(name))
            .cloned()
    };

    if opts.notebooks {
        if let Some(nb) = folder(".notebooks") {
            for e in kindle.list(Some(nb.handle))? {
                if !e.is_folder {
                    continue;
                }
                if e.name == "thumbnails" {
                    for t in kindle.list(Some(e.handle))? {
                        if let Some(stem) = t.name.strip_suffix(".png").filter(|s| is_uuid(s)) {
                            let local = layout.notebooks_rel().join(format!("{stem}.png"));
                            found.push((Kind::Thumbnail, format!(".notebooks/thumbnails/{}", t.name), t, local));
                        }
                    }
                } else if is_uuid(&e.name) || e.name.contains("!!") {
                    let kind = if is_uuid(&e.name) {
                        Kind::Notebook
                    } else {
                        Kind::Annotation
                    };
                    if let Some(nbk) = kindle.list(Some(e.handle))?.into_iter().find(|f| f.name == "nbk") {
                        let local = match kind {
                            Kind::Notebook => layout.notebooks_rel().join(format!("{}.nbk", e.name)),
                            _ => layout
                                .annotations_rel()
                                .join(format!("{}.nbk", safe_component(&e.name))),
                        };
                        found.push((kind, format!(".notebooks/{}/nbk", e.name), nbk, local));
                    }
                }
            }
        } else {
            log("no .notebooks folder on the device".into());
        }
        if opts.sync_db
            && let Some(sync) = folder(".sync")
            && let Some(db) = kindle
                .list(Some(sync.handle))?
                .into_iter()
                .find(|f| f.name == "KSDKNoteSyncDB.sqlite")
        {
            found.push((
                Kind::SyncDb,
                ".sync/KSDKNoteSyncDB.sqlite".into(),
                db,
                layout.sync_db_rel(),
            ));
        }
    }

    if opts.books
        && let Some(docs) = folder("documents")
    {
        // documents/, keeping subfolders; .sdr sidecar folders come along whole
        let mut stack: Vec<(Handle, Vec<String>)> = vec![(docs.handle, Vec::new())];
        while let Some((h, path)) = stack.pop() {
            for e in kindle.list(Some(h))? {
                let mut p = path.clone();
                p.push(e.name.clone());
                if e.is_folder {
                    // dot-folders (.cache) hold device caches, not books
                    if p.len() <= 8 && !e.name.starts_with('.') {
                        stack.push((e.handle, p));
                    }
                    continue;
                }
                let sdr = path.iter().position(|c| c.to_ascii_lowercase().ends_with(".sdr"));
                let kind = match sdr {
                    Some(_) => Kind::BookSidecar,
                    None if BOOK_EXTENSIONS.contains(&ext(&e.name).as_str()) => Kind::Book,
                    None => continue,
                };
                // class from the book (or, for sidecars, the book the .sdr belongs to)
                let class = match sdr {
                    Some(i) => {
                        let stem = path[i].rsplit_once('.').map_or(path[i].as_str(), |(s, _)| s);
                        classify_book(stem, if i > 0 { &path[i - 1] } else { "" })
                    }
                    None => {
                        let stem = e.name.rsplit_once('.').map_or(e.name.as_str(), |(s, _)| s);
                        classify_book(stem, path.last().map_or("", String::as_str))
                    }
                };
                let mut local = layout.books_rel(class);
                for c in &p {
                    local.push(safe_component(c));
                }
                found.push((kind, format!("documents/{}", p.join("/")), e, local));
            }
        }
    }

    if opts.screenshots {
        let mut shots: Vec<(String, Entry)> = root
            .iter()
            .filter(|e| !e.is_folder && IMAGE_EXTENSIONS.contains(&ext(&e.name).as_str()))
            .map(|e| (e.name.clone(), e.clone()))
            .collect();
        if let Some(dir) = folder("screenshots") {
            for e in kindle.list(Some(dir.handle))? {
                if !e.is_folder && IMAGE_EXTENSIONS.contains(&ext(&e.name).as_str()) {
                    shots.push((format!("screenshots/{}", e.name), e));
                }
            }
        }
        for (path, e) in shots {
            let local = layout.screenshots_rel().join(safe_component(&e.name));
            found.push((Kind::Screenshot, path, e, local));
        }
    }

    // Decide, reading SQLite headers where sizes alone cannot tell.
    let mut items = Vec::with_capacity(found.len());
    let mut changed_notebooks = std::collections::HashSet::new();
    for (kind, device_path, e, local) in found {
        let abs = layout.root.join(&local);
        let counter = if matches!(kind, Kind::Notebook | Kind::Annotation | Kind::SyncDb) {
            kindle.read_head(e.handle, 100).ok().and_then(|h| sqlite_counter(&h))
        } else {
            None
        };
        let local_size = std::fs::metadata(&abs).ok().map(|m| m.len());
        // The Python app put every book straight into books/<class>/, without
        // the Kindle's subfolders. Reuse such a copy if it is the same size.
        let relocate_from = match (local_size, kind) {
            (None, Kind::Book | Kind::BookSidecar) => local
                .parent()
                .and_then(|p| p.ancestors().nth(p.components().count().saturating_sub(2)))
                .map(|class_dir| class_dir.join(local.file_name().unwrap_or_default()))
                .filter(|old| *old != local)
                .filter(|old| std::fs::metadata(layout.root.join(old)).is_ok_and(|m| m.len() == e.size)),
            _ => None,
        };
        let (action, reason) = match local_size {
            None if relocate_from.is_some() => (
                Action::New,
                Some(format!(
                    "already in the library at {}; moved",
                    relocate_from.as_ref().unwrap().display()
                )),
            ),
            None => (Action::New, None),
            Some(_) if opts.force => (Action::Changed, Some("forced".to_string())),
            Some(s) if s != e.size => (Action::Changed, Some(format!("size {s} → {}", e.size))),
            Some(_) => match (counter, local_counter(&abs)) {
                (Some(d), Some(l)) if d != l => (Action::Changed, Some(format!("edited (change counter {l} → {d})"))),
                _ => (Action::UpToDate, None),
            },
        };
        if kind == Kind::Notebook && action != Action::UpToDate {
            changed_notebooks.insert(local.with_extension(""));
        }
        items.push(SyncItem {
            kind,
            device_path,
            handle: e.handle,
            size: e.size,
            counter,
            local,
            action,
            reason,
            relocate_from,
        });
    }
    // a changed notebook gets a fresh thumbnail even if its size happens to match
    for i in items.iter_mut() {
        if i.kind == Kind::Thumbnail
            && i.action == Action::UpToDate
            && changed_notebooks.contains(&i.local.with_extension(""))
        {
            i.action = Action::Changed;
            i.reason = Some("notebook changed".into());
        }
    }
    items.retain(|i| opts.wants(i.kind.category()));
    Ok(SyncPlan { items })
}

/// Copies what `plan` marks as new or changed. `progress(item, done, total)` is
/// called during each download. A failed file is logged and skipped.
pub fn run(
    kindle: &Kindle,
    layout: &Layout,
    plan: &SyncPlan,
    progress: &mut dyn FnMut(&SyncItem, u64, u64),
    cancel: Option<&AtomicBool>,
    log: &mut dyn FnMut(String),
) -> Result<SyncReport> {
    let start = Instant::now();
    let mut report = SyncReport {
        device: kindle.info().name.clone(),
        ..SyncReport::default()
    };
    let mut manifest = Manifest::load(layout);
    let mut say = |report: &mut SyncReport, msg: String| {
        log(msg.clone());
        report.log.push(msg);
    };

    for item in &plan.items {
        let primary = matches!(
            item.kind,
            Kind::Notebook | Kind::Annotation | Kind::Book | Kind::Screenshot
        );
        if primary {
            *report.found.entry(item.kind.category()).or_default() += 1;
        }
        let key = item.local.to_string_lossy().into_owned();
        if item.action == Action::UpToDate {
            if primary {
                *report.skipped.entry(item.kind.category()).or_default() += 1;
            }
            manifest.files.entry(key).or_insert(ManifestEntry {
                kind: item.kind,
                device_path: item.device_path.clone(),
                size: item.size,
                counter: item.counter,
                synced_at: now(),
            });
            continue;
        }
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            say(&mut report, "cancelled".into());
            break;
        }
        let dest = layout.root.join(&item.local);
        let why = item.reason.as_deref().map(|r| format!(" ({r})")).unwrap_or_default();
        if let Some(old) = &item.relocate_from {
            let moved = dest
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|_| std::fs::rename(layout.root.join(old), &dest));
            match moved {
                Ok(()) => {
                    if primary {
                        *report.copied.entry(item.kind.category()).or_default() += 1;
                    }
                    say(
                        &mut report,
                        format!("→ {} → {}{why}", old.display(), item.local.display()),
                    );
                    manifest.files.insert(
                        key,
                        ManifestEntry {
                            kind: item.kind,
                            device_path: item.device_path.clone(),
                            size: item.size,
                            counter: item.counter,
                            synced_at: now(),
                        },
                    );
                    continue;
                }
                Err(e) => say(
                    &mut report,
                    format!("could not move {} ({e}); downloading instead", old.display()),
                ),
            }
        }
        // Empty files are written without asking the Kindle: some (the `res/rsrc8`
        // placeholders of device-converted PDFs) have object info mtp-rs can't parse.
        let fetched = if item.size == 0 {
            dest.parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|_| std::fs::write(&dest, b""))
                .map(|_| 0)
                .map_err(DeviceError::from)
        } else {
            kindle.download(item.handle, &dest, &mut |d, t| progress(item, d, t), cancel)
        };
        match fetched {
            Ok(n) => {
                report.bytes_copied += n;
                if primary {
                    *report.copied.entry(item.kind.category()).or_default() += 1;
                }
                say(
                    &mut report,
                    format!("↓ {} → {}{why}", item.device_path, item.local.display()),
                );
                manifest.files.insert(
                    key,
                    ManifestEntry {
                        kind: item.kind,
                        device_path: item.device_path.clone(),
                        size: n,
                        counter: item.counter,
                        synced_at: now(),
                    },
                );
            }
            Err(DeviceError::Cancelled) => {
                say(&mut report, "cancelled".into());
                break;
            }
            Err(e) => {
                let msg = format!("{}: {e}", item.device_path);
                say(&mut report, format!("✗ {msg}"));
                report.errors.push(msg);
            }
        }
    }
    manifest.save(layout)?;
    report.seconds = start.elapsed().as_secs_f64();
    Ok(report)
}
