//! Long operations, each started as a job: sync, push, pull, delete,
//! "Free the Kindle", export and migration from the old library.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use folio_device::Kindle;
use folio_library::index::ItemKind;
use folio_library::sync::{self, SyncOptions};
use folio_library::transfer::{self, DeleteMode};
use folio_library::{classify_book, safe_component};
use folio_pdfout::LONG_EDGE_PT;
use folio_render::{PencilMode, RasterOptions};
use serde_json::{Value, json};

use crate::app::{App, Direction, JobState, Result, RetrySpec, device_error};
use crate::export::{ExportMode, ExportOptions, export_pdf};

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Bytes per second over the whole transfer so far.
fn rate(start: Instant, done: u64) -> f64 {
    done as f64 / start.elapsed().as_secs_f64().max(0.001)
}

pub fn sync_preview(app: &Arc<App>, opts: SyncOptions) -> Result<Value> {
    app.with_kindle(|k| {
        let plan = sync::plan(k, &app.layout, &opts, &mut |_| {}).map_err(err)?;
        let items: Vec<Value> = plan
            .to_copy()
            .map(|i| json!({"kind": i.kind, "device_path": i.device_path, "size": i.size, "action": i.action, "reason": i.reason}))
            .collect();
        Ok(json!({"summary": plan.summary(), "to_copy": items}))
    })
}

pub fn start_sync(app: &Arc<App>, mut opts: SyncOptions) -> u64 {
    // the ~1 GB sync database only when opted in (Settings › Sync)
    opts.sync_db |= app.setting("sync_db") == "true";
    let what: Vec<&str> = [
        (opts.notebooks, "notebooks"),
        (opts.books, "books"),
        (opts.screenshots, "screenshots"),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .map(|(_, n)| *n)
    .collect();
    app.spawn_job("sync", format!("Sync {}", what.join(", ")), move |ctx| {
        let app = ctx.app.clone();
        let report = app.with_kindle(|k| {
            ctx.log(format!("Connected: {}", k.info().name));
            ctx.progress(0, 0, "Scanning the Kindle…");
            let plan = sync::plan(k, &app.layout, &opts, &mut |m| ctx.log(m)).map_err(err)?;
            for (cat, s) in plan.summary() {
                ctx.log(format!(
                    "{cat:?}: {} found, {} new, {} changed, {} up to date",
                    s.found, s.new, s.changed, s.up_to_date
                ));
            }
            let total: u64 = plan.to_copy().map(|i| i.size).sum();
            let mut transfer_ids: HashMap<String, (u64, Instant)> = HashMap::new();
            for i in plan.to_copy() {
                let name = i.device_path.rsplit('/').next().unwrap_or(&i.device_path).to_string();
                let tid = app.add_transfer(
                    ctx.id,
                    Direction::Down,
                    &name,
                    i.size,
                    Some(RetrySpec::Pull {
                        device_path: i.device_path.clone(),
                    }),
                );
                transfer_ids.insert(i.device_path.clone(), (tid, Instant::now()));
            }
            let mut finished_bytes = 0u64;
            let mut current: Option<String> = None;
            let report = sync::run(
                k,
                &app.layout,
                &plan,
                &mut |item, done, size| {
                    if current.as_deref() != Some(&item.device_path) {
                        if let Some(prev) = current.take()
                            && let Some((tid, _)) = transfer_ids.get(&prev)
                        {
                            app.update_transfer(*tid, |t| t.state = JobState::Done);
                            finished_bytes += size_of(&plan, &prev);
                        }
                        current = Some(item.device_path.clone());
                        if let Some((tid, start)) = transfer_ids.get_mut(&item.device_path) {
                            *start = Instant::now();
                            app.update_transfer(*tid, |t| t.state = JobState::Running);
                        }
                    }
                    if let Some((tid, start)) = transfer_ids.get(&item.device_path) {
                        let speed = rate(*start, done);
                        app.update_transfer(*tid, |t| {
                            t.done = done;
                            t.bytes_per_sec = speed;
                        });
                    }
                    ctx.progress(finished_bytes + done.min(size), total, item.device_path.to_string());
                },
                Some(&ctx.cancel),
                &mut |m| ctx.log(m),
            )
            .map_err(err)?;
            for (path, (tid, _)) in &transfer_ids {
                let failed = report.errors.iter().find(|e| e.starts_with(path.as_str()));
                app.update_transfer(*tid, |t| {
                    if t.state != JobState::Done {
                        t.state = match (failed, ctx.cancelled()) {
                            (Some(_), _) => JobState::Failed,
                            (None, true) if t.done < t.size => JobState::Cancelled,
                            _ => JobState::Done,
                        };
                        if t.state == JobState::Done {
                            t.done = t.size;
                        }
                        t.error = failed.cloned();
                    }
                });
            }
            Ok(report)
        })?;
        app.scan()?;
        ctx.log(format!(
            "Done: {:.1} MB in {:.1} s",
            report.bytes_copied as f64 / 1e6,
            report.seconds
        ));
        if !report.errors.is_empty() {
            return Err(format!(
                "{} files failed: {}",
                report.errors.len(),
                report.errors.join("; ")
            ));
        }
        serde_json::to_value(&report).map_err(err)
    })
}

fn size_of(plan: &sync::SyncPlan, device_path: &str) -> u64 {
    plan.items
        .iter()
        .find(|i| i.device_path == device_path)
        .map_or(0, |i| i.size)
}

pub fn start_push(app: &Arc<App>, paths: Vec<PathBuf>, to: String) -> u64 {
    let title = match paths.as_slice() {
        [one] => format!("Send {}", one.file_name().unwrap_or_default().to_string_lossy()),
        many => format!("Send {} files", many.len()),
    };
    let to2 = to.clone();
    let app2 = app.clone();
    app.spawn_job("push", title, move |ctx| {
        let app = app2;
        let ids: Vec<(PathBuf, u64)> = paths
            .iter()
            .map(|p| {
                let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                (
                    p.clone(),
                    app.add_transfer(
                        ctx.id,
                        Direction::Up,
                        &name,
                        size,
                        Some(RetrySpec::Push {
                            path: p.clone(),
                            to: to2.clone(),
                        }),
                    ),
                )
            })
            .collect();
        let mut sent = Vec::new();
        let mut failures = Vec::new();
        app.with_kindle(|k| {
            for (path, tid) in &ids {
                if ctx.cancelled() {
                    app.update_transfer(*tid, |t| t.state = JobState::Cancelled);
                    continue;
                }
                app.update_transfer(*tid, |t| t.state = JobState::Running);
                let start = Instant::now();
                let tid2 = *tid;
                let app3 = app.clone();
                let r = transfer::push(
                    k,
                    path,
                    &to,
                    &mut move |done, total| {
                        let speed = rate(start, done);
                        app3.update_transfer(tid2, |t| {
                            t.done = done;
                            t.size = total;
                            t.bytes_per_sec = speed;
                        });
                    },
                    Some(&ctx.cancel),
                );
                match r {
                    Ok(res) => {
                        ctx.log(format!("↑ {} → {}", path.display(), res.device_path));
                        app.update_transfer(*tid, |t| {
                            t.state = JobState::Done;
                            t.done = t.size;
                        });
                        sent.push(res.device_path);
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        ctx.log(format!("✗ {}: {msg}", path.display()));
                        app.update_transfer(*tid, |t| {
                            t.state = if ctx.cancelled() {
                                JobState::Cancelled
                            } else {
                                JobState::Failed
                            };
                            t.error = Some(msg.clone());
                        });
                        failures.push(format!("{}: {msg}", path.display()));
                    }
                }
            }
            Ok(())
        })?;
        if !failures.is_empty() {
            return Err(failures.join("; "));
        }
        Ok(json!({ "sent": sent }))
    })
}

/// Where a pulled file goes: books into the library, anything else to ~/Downloads.
fn pull_destination(app: &App, device_path: &str) -> PathBuf {
    let parts: Vec<&str> = device_path.split('/').filter(|p| !p.is_empty()).collect();
    let name = parts.last().copied().unwrap_or("file");
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    let is_book = [
        "pdf", "epub", "kfx", "azw", "azw3", "azw8", "mobi", "prc", "txt", "docx",
    ]
    .contains(&ext.as_str());
    if parts.first().is_some_and(|p| p.eq_ignore_ascii_case("documents")) && is_book && parts.len() >= 2 {
        let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
        let parent = if parts.len() >= 3 { parts[parts.len() - 2] } else { "" };
        let mut p = app.layout.root.join(app.layout.books_rel(classify_book(stem, parent)));
        for c in &parts[1..] {
            p.push(safe_component(c));
        }
        return p;
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join("Downloads").join(safe_component(name))
}

pub fn start_pull(app: &Arc<App>, device_path: String) -> u64 {
    let name = device_path.rsplit('/').next().unwrap_or(&device_path).to_string();
    app.spawn_job("pull", format!("Copy {name} from the Kindle"), move |ctx| {
        let app = ctx.app.clone();
        let dest = pull_destination(&app, &device_path);
        let entry = app
            .with_kindle(|k| k.resolve(&device_path).map_err(err))?
            .ok_or_else(|| format!("{device_path} is not on the Kindle"))?;
        let tid = app.add_transfer(
            ctx.id,
            Direction::Down,
            &name,
            entry.size,
            Some(RetrySpec::Pull {
                device_path: device_path.clone(),
            }),
        );
        app.update_transfer(tid, |t| t.state = JobState::Running);
        let start = Instant::now();
        let r = app.with_kindle(|k| {
            k.download(
                entry.handle,
                &dest,
                &mut |done, total| {
                    let speed = rate(start, done);
                    app.update_transfer(tid, |t| {
                        t.done = done;
                        t.size = total;
                        t.bytes_per_sec = speed;
                    });
                    ctx.progress(done, total, name.clone());
                },
                Some(&ctx.cancel),
            )
            .map_err(err)
        });
        app.update_transfer(tid, |t| match &r {
            Ok(_) => t.state = JobState::Done,
            Err(e) => {
                t.state = if ctx.cancelled() {
                    JobState::Cancelled
                } else {
                    JobState::Failed
                };
                t.error = Some(e.clone());
            }
        });
        r?;
        app.scan()?;
        Ok(json!({ "path": dest }))
    })
}

pub fn retry_transfer(app: &Arc<App>, transfer: u64) -> Result<u64> {
    match app.transfer_retry(transfer) {
        Some(RetrySpec::Push { path, to }) => Ok(start_push(app, vec![path], to)),
        Some(RetrySpec::Pull { device_path }) => Ok(start_pull(app, device_path)),
        None => Err("this transfer cannot be retried".into()),
    }
}

pub fn device_ls(app: &Arc<App>, path: &str) -> Result<Value> {
    app.with_kindle(|k| {
        let parent = match path.trim_matches('/') {
            "" => None,
            p => Some(
                k.resolve(p)
                    .map_err(err)?
                    .ok_or_else(|| format!("{p} is not on the Kindle"))?
                    .handle,
            ),
        };
        let mut entries = k.list(parent).map_err(err)?;
        entries.sort_by_key(|a| (!a.is_folder, a.name.to_lowercase()));
        Ok(json!(
            entries
                .iter()
                .map(|e| json!({"name": e.name, "size": e.size, "folder": e.is_folder}))
                .collect::<Vec<_>>()
        ))
    })
}

pub fn device_mkdir(app: &Arc<App>, path: &str) -> Result<()> {
    let path = path.trim_matches('/');
    if !path.to_ascii_lowercase().starts_with("documents/") {
        return Err("new folders go inside documents/".into());
    }
    let (parent, name) = path.rsplit_once('/').ok_or("bad folder name")?;
    app.with_kindle(|k| {
        let p = k
            .resolve(parent)
            .map_err(err)?
            .ok_or_else(|| format!("{parent} is not on the Kindle"))?;
        k.mkdir(Some(p.handle), name).map_err(err)?;
        Ok(())
    })
}

/// Deletes a book or screenshot on the Kindle (then locally), or only locally.
/// Files synced by the Python app have no manifest entry; their device path is
/// worked out from the library path and checked by size before deleting.
pub fn delete_item(app: &Arc<App>, id: &str, local_only: bool) -> Result<Value> {
    let item = app.item(id)?;
    if item.kind == ItemKind::Notebook {
        return Err("notebooks are managed on the Kindle".into());
    }
    let mode = if local_only {
        DeleteMode::LocalOnly
    } else {
        DeleteMode::DeviceThenLocal
    };
    let manifest_has = folio_library::sync::Manifest::load(&app.layout)
        .files
        .contains_key(&item.rel_path);
    let result = if manifest_has {
        if local_only {
            transfer::delete(None, &app.layout, &item.rel_path, mode).map_err(err)?
        } else {
            app.with_kindle(|k| transfer::delete(Some(k), &app.layout, &item.rel_path, mode).map_err(err))?
        }
    } else {
        let local = app.item_path(&item);
        let device_path = if local_only {
            None
        } else {
            Some(app.with_kindle(|k| delete_unrecorded(k, &item.rel_path, item.size))?)
        };
        std::fs::remove_file(&local).map_err(err)?;
        transfer::DeleteResult {
            device_path: device_path.clone().unwrap_or_default(),
            deleted_on_device: device_path.is_some(),
            removed_locally: true,
        }
    };
    app.scan()?;
    serde_json::to_value(result).map_err(err)
}

fn delete_unrecorded(k: &Kindle, rel: &str, size: u64) -> Result<String> {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let candidates: Vec<String> = if let Some(rest) = rel.strip_prefix("screenshots/") {
        vec![format!("screenshots/{rest}"), rest.to_string()]
    } else if let Some(rest) = rel
        .strip_prefix("books/purchased/")
        .or_else(|| rel.strip_prefix("books/sideloaded/"))
    {
        vec![format!("documents/{rest}"), format!("documents/{name}")]
    } else {
        vec![]
    };
    for c in candidates {
        if let Some(e) = k.resolve(&c).map_err(err)? {
            if e.is_folder || e.size != size {
                return Err(format!(
                    "{c} on the Kindle is not the same file ({} bytes, local {size}); not deleting it",
                    e.size
                ));
            }
            k.delete(e.handle).map_err(err)?;
            return Ok(c);
        }
    }
    Err(format!("{name} was not found on the Kindle; use “Remove locally”"))
}

pub fn start_free(app: &Arc<App>) -> u64 {
    app.spawn_job("free", "Free the Kindle", |ctx| {
        let app = ctx.app.clone();
        let _session = app
            .device_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ctx.progress(0, 0, "Asking macOS to let go of the Kindle…");
        match folio_device::free_kindle(Duration::from_secs(10)) {
            Ok(k) => {
                app.set_device(crate::app::status_of(&k));
                let _ = k.close();
                Ok(json!({"freed": true}))
            }
            Err(e) => Err(device_error(&e)),
        }
    })
}

#[derive(Clone, Debug)]
pub struct ExportRequest {
    pub mode: String,
    pub long_edge: u32,
    pub pencil_scale: u32,
    pub shapes: bool,
    pub pages: Option<Vec<usize>>,
    pub dest_dir: PathBuf,
    pub force: bool,
}

pub fn default_export_dir(app: &App) -> PathBuf {
    let s = app.setting("export_dir");
    if !s.is_empty() {
        return PathBuf::from(s);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join("Documents/Folio Exports")
}

/// One job per notebook; they queue behind each other and cancel separately.
pub fn start_export(app: &Arc<App>, id: String, req: ExportRequest) -> Result<u64> {
    let item = app.item(&id)?;
    if item.kind != ItemKind::Notebook {
        return Err("only notebooks are exported to PDF".into());
    }
    let name = format!("{}.pdf", safe_component(&item.title));
    let dest = req.dest_dir.join(&name);
    Ok(app.spawn_job("export", format!("Export {}", item.title), move |ctx| {
        let app = ctx.app.clone();
        let _turn = app.export_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if ctx.cancelled() {
            return Err("cancelled".into());
        }
        let src = app.item_path(&item);
        let src_time = std::fs::metadata(&src).and_then(|m| m.modified()).ok();
        let dst_time = std::fs::metadata(&dest).and_then(|m| m.modified()).ok();
        if !req.force && req.pages.is_none() && src_time.is_some() && dst_time >= src_time {
            ctx.log(format!("{} is up to date", dest.display()));
            return Ok(json!({"path": dest, "skipped": true}));
        }
        std::fs::create_dir_all(&req.dest_dir).map_err(err)?;
        let mode = if req.mode == "raster" {
            ExportMode::Raster(RasterOptions {
                long_edge: req.long_edge,
                pencil: PencilMode::Texture { scale: req.pencil_scale },
                shapes: req.shapes,
            })
        } else {
            ExportMode::Vector { shapes: req.shapes }
        };
        let opts = ExportOptions { mode, jobs: 0, pages: req.pages.clone(), cache_dir: Some(app.cache.root.join("kdf")) };
        let stats = export_pdf(&src, &dest, &opts, Some(&ctx.cancel), &|done, total| {
            ctx.progress(done as u64, total as u64, format!("page {done} of {total}"));
        })
        .map_err(err)?;
        Ok(json!({"path": dest, "pages": stats.pages, "bytes": stats.bytes, "seconds": stats.elapsed.as_secs_f64(), "page_pt": LONG_EDGE_PT}))
    }))
}

/// Copies an old `scribe-library/library` (the earlier Python app) folder into this library
/// (same layout), skipping files that are already here with the same size.
pub fn start_migrate(app: &Arc<App>, from: PathBuf) -> u64 {
    app.spawn_job("migrate", format!("Import {}", from.display()), move |ctx| {
        let app = ctx.app.clone();
        let mut files = Vec::new();
        for sub in ["notebooks", "books", "screenshots"] {
            let mut stack = vec![from.join(sub)];
            while let Some(dir) = stack.pop() {
                let Ok(rd) = std::fs::read_dir(&dir) else { continue };
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else {
                        files.push(p);
                    }
                }
            }
        }
        if files.is_empty() {
            return Err(format!("{} has no notebooks/, books/ or screenshots/", from.display()));
        }
        let (mut copied, mut skipped) = (0, 0);
        for (n, src) in files.iter().enumerate() {
            if ctx.cancelled() {
                break;
            }
            let rel = src.strip_prefix(&from).unwrap_or(src);
            let dst = app.layout.root.join(rel);
            let same = std::fs::metadata(&dst).ok().map(|m| m.len()) == std::fs::metadata(src).ok().map(|m| m.len());
            if same {
                skipped += 1;
            } else {
                copy_into(src, &dst).map_err(err)?;
                copied += 1;
            }
            ctx.progress(n as u64 + 1, files.len() as u64, rel.display().to_string());
        }
        app.scan()?;
        Ok(json!({"copied": copied, "skipped": skipped}))
    })
}

fn copy_into(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(d) = dst.parent() {
        std::fs::create_dir_all(d)?;
    }
    let part = dst.with_extension("part-import");
    std::fs::copy(src, &part)?;
    std::fs::rename(part, dst)
}

/// Deletes a file in `documents/` or `screenshots/` on the Kindle (device
/// browser), then its copy in the library if sync made one.
pub fn device_delete(app: &Arc<App>, path: &str) -> Result<Value> {
    let p = path.trim_matches('/').to_string();
    let lower = p.to_ascii_lowercase();
    if !(lower.starts_with("documents/") || lower.starts_with("screenshots/")) {
        return Err("only files in documents/ and screenshots/ can be deleted here".into());
    }
    app.with_kindle(|k| {
        let e = k
            .resolve(&p)
            .map_err(err)?
            .ok_or_else(|| format!("{p} is not on the Kindle"))?;
        if e.is_folder {
            return Err("folders are not deleted from here".into());
        }
        k.delete(e.handle).map_err(err)?;
        if k.resolve(&p).map_err(err)?.is_some() {
            return Err(format!("{p} is still on the Kindle after deleting it"));
        }
        Ok(())
    })?;
    let mut manifest = sync::Manifest::load(&app.layout);
    let local = manifest
        .files
        .iter()
        .find(|(_, e)| e.device_path == p)
        .map(|(k, _)| k.clone());
    let mut removed_locally = false;
    if let Some(local) = local {
        removed_locally = std::fs::remove_file(app.layout.root.join(&local)).is_ok();
        manifest.files.remove(&local);
        manifest.save(&app.layout).map_err(err)?;
        app.scan()?;
    }
    Ok(json!({"deleted": p, "removed_locally": removed_locally}))
}

/// Turns macOS's ptpcamerad off (or on) for this user (Settings → Device; opt-in).
pub fn set_ptpcamerad(app: &Arc<App>, disable: bool) -> Result<()> {
    let uid = String::from_utf8_lossy(
        &std::process::Command::new("/usr/bin/id")
            .arg("-u")
            .output()
            .map_err(err)?
            .stdout,
    )
    .trim()
    .to_string();
    let service = format!("gui/{uid}/com.apple.ptpcamerad");
    let verb = if disable { "disable" } else { "enable" };
    let status = std::process::Command::new("/bin/launchctl")
        .args([verb, &service])
        .status()
        .map_err(err)?;
    if !status.success() {
        return Err(format!("launchctl {verb} {service} failed"));
    }
    if disable {
        let _ = std::process::Command::new("/usr/bin/pkill")
            .args(["-TERM", "-x", "ptpcamerad"])
            .status();
    }
    app.set_setting("ptpcamerad_disabled", if disable { "true" } else { "false" })
}
