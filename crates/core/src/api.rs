//! The one entry point the UI talks to: `call(name, args)` for commands and
//! `fetch(path)` for page images and files. The Tauri shell and the dev bridge
//! (`folio-cli serve`) both route here, so they behave the same.

use std::path::PathBuf;
use std::sync::Arc;

use folio_library::index::ItemKind;
use folio_library::sync::SyncOptions;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::{App, Result, counts};
use crate::ops::{self, ExportRequest};

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn args<T: for<'de> Deserialize<'de>>(v: Value) -> Result<T> {
    serde_json::from_value(v).map_err(|e| format!("bad arguments: {e}"))
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize, Default)]
struct SyncArgs {
    #[serde(default)]
    notebooks: bool,
    #[serde(default)]
    books: bool,
    #[serde(default)]
    screenshots: bool,
    #[serde(default)]
    force: bool,
}

impl SyncArgs {
    fn options(&self) -> SyncOptions {
        let any = self.notebooks || self.books || self.screenshots;
        SyncOptions {
            notebooks: self.notebooks || !any,
            books: self.books || !any,
            screenshots: self.screenshots || !any,
            force: self.force,
            sync_db: false,
        }
    }
}

#[derive(Deserialize)]
struct ExportArgs {
    ids: Vec<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    long_edge: Option<u32>,
    #[serde(default)]
    pencil_scale: Option<u32>,
    #[serde(default)]
    shapes: Option<bool>,
    /// 1-based, e.g. "1-10, 15"; only for a single notebook.
    #[serde(default)]
    pages: Option<String>,
    #[serde(default)]
    dest_dir: Option<String>,
    #[serde(default)]
    force: bool,
}

fn parse_pages(spec: &str) -> Result<Vec<usize>> {
    let mut out = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (
                a.trim().parse::<usize>().map_err(err)?,
                b.trim().parse::<usize>().map_err(err)?,
            ),
            None => {
                let n = part.parse::<usize>().map_err(err)?;
                (n, n)
            }
        };
        if a == 0 || b < a {
            return Err(format!("bad page range “{part}”"));
        }
        out.extend(a - 1..b);
    }
    Ok(out)
}

pub fn call(app: &Arc<App>, name: &str, a: Value) -> Result<Value> {
    match name {
        "status" => Ok(json!({
            "library_root": app.layout.root,
            "cache_root": app.cache.root,
            "counts": counts(app),
            "device": app.device_status(),
            "cache_bytes": app.cache_bytes(),
            "jobs": app.jobs(),
            "transfers": app.transfers(),
        })),
        "items" => {
            #[derive(Deserialize)]
            struct A {
                kind: Option<ItemKind>,
            }
            let a: A = args(a)?;
            Ok(serde_json::to_value(app.with_index(|i| i.items(a.kind))?).map_err(err)?)
        }
        "rescan" => Ok(json!(app.scan()?)),
        "notebook" => {
            let Id { id } = args(a)?;
            let item = app.item(&id)?;
            let nb = app.notebook(&item)?;
            let nb = nb.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let pages: Vec<(i64, i64)> = nb
                .outline()
                .pages()
                .map(|p| p.canvas.unwrap_or((15624, 20832)))
                .collect();
            let position = app.with_index(|i| i.position(&id))?;
            Ok(json!({"item": item, "pages": pages, "position": position}))
        }
        "position" => {
            let Id { id } = args(a)?;
            Ok(json!(app.with_index(|i| i.position(&id))?))
        }
        "set_position" => {
            #[derive(Deserialize)]
            struct A {
                id: String,
                page: u32,
            }
            let a: A = args(a)?;
            app.with_index(|i| i.set_position(&a.id, a.page))?;
            app.set_setting("last_item", &a.id)?;
            Ok(Value::Null)
        }
        "search_text" => {
            #[derive(Deserialize)]
            struct A {
                query: String,
                limit: Option<usize>,
            }
            let a: A = args(a)?;
            let hits = app.with_index(|i| i.search_text(&a.query, a.limit.unwrap_or(100)))?;
            let (books, chunks) = app.with_index(|i| i.text_stats())?;
            Ok(json!({"hits": hits, "books_indexed": books, "chunks": chunks}))
        }
        "export_annotated" => {
            #[derive(Deserialize)]
            struct A {
                id: String,
                dest: PathBuf,
            }
            let a: A = args(a)?;
            let pages = crate::annotations::export_pdf(app, &a.id, &a.dest)?;
            Ok(json!({"pages": pages, "path": a.dest}))
        }
        "book_annotations" => {
            let Id { id } = args(a)?;
            Ok(json!(crate::annotations::pages(app, &id)?))
        }
        "location" => {
            let Id { id } = args(a)?;
            let loc = app.with_index(|i| i.location(&id))?;
            Ok(json!(loc.map(
                |(location, fraction)| json!({"location": location, "fraction": fraction})
            )))
        }
        "set_location" => {
            #[derive(Deserialize)]
            struct A {
                id: String,
                location: String,
                fraction: Option<f64>,
            }
            let a: A = args(a)?;
            app.with_index(|i| i.set_location(&a.id, &a.location, a.fraction))?;
            app.set_setting("last_item", &a.id)?;
            Ok(Value::Null)
        }
        "rename" => {
            #[derive(Deserialize)]
            struct A {
                id: String,
                title: Option<String>,
            }
            let a: A = args(a)?;
            app.with_index(|i| i.rename(&a.id, a.title.as_deref()))?;
            app.emit(crate::app::Event::Library);
            Ok(Value::Null)
        }
        "settings" => Ok(json!({"values": app.settings(), "export_dir_default": ops::default_export_dir(app)})),
        "set_setting" => {
            #[derive(Deserialize)]
            struct A {
                key: String,
                value: String,
            }
            let a: A = args(a)?;
            app.set_setting(&a.key, &a.value)?;
            Ok(Value::Null)
        }
        "clear_cache" => Ok(json!(app.clear_cache()?)),
        "jobs" => Ok(json!(app.jobs())),
        "cancel_job" => {
            #[derive(Deserialize)]
            struct A {
                id: u64,
            }
            let a: A = args(a)?;
            Ok(json!(app.cancel_job(a.id)))
        }
        "transfers" => Ok(json!(app.transfers())),
        "retry_transfer" => {
            #[derive(Deserialize)]
            struct A {
                id: u64,
            }
            let a: A = args(a)?;
            Ok(json!(ops::retry_transfer(app, a.id)?))
        }
        "clear_transfers" => {
            app.clear_finished_transfers();
            Ok(Value::Null)
        }
        "export" => {
            let a: ExportArgs = args(a)?;
            let pages = match a.pages.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
                Some(p) if a.ids.len() == 1 => Some(parse_pages(p)?),
                Some(_) => return Err("a page range needs a single notebook".into()),
                None => None,
            };
            let req = ExportRequest {
                mode: a.mode.unwrap_or_else(|| app.setting("export_mode")),
                long_edge: a
                    .long_edge
                    .unwrap_or_else(|| app.setting("export_long_edge").parse().unwrap_or(2480)),
                pencil_scale: a
                    .pencil_scale
                    .unwrap_or_else(|| app.setting("pencil_scale").parse().unwrap_or(4)),
                shapes: a.shapes.unwrap_or_else(|| app.setting("draw_shapes") != "false"),
                pages,
                dest_dir: a
                    .dest_dir
                    .filter(|d| !d.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| ops::default_export_dir(app)),
                force: a.force,
            };
            let ids: Vec<u64> = a
                .ids
                .into_iter()
                .map(|id| ops::start_export(app, id, req.clone()))
                .collect::<Result<_>>()?;
            Ok(json!(ids))
        }
        "device_status" => Ok(json!(app.refresh_device())),
        "free_kindle" => Ok(json!(ops::start_free(app))),
        "sync_preview" => {
            let a: SyncArgs = args(a)?;
            ops::sync_preview(app, a.options())
        }
        "sync" => {
            let a: SyncArgs = args(a)?;
            Ok(json!(ops::start_sync(app, a.options())))
        }
        "push" => {
            #[derive(Deserialize)]
            struct A {
                paths: Vec<PathBuf>,
                #[serde(default)]
                to: Option<String>,
            }
            let a: A = args(a)?;
            if a.paths.is_empty() {
                return Err("no files to send".into());
            }
            Ok(json!(ops::start_push(
                app,
                a.paths,
                a.to.unwrap_or_else(|| "documents".into())
            )))
        }
        "delete" => {
            #[derive(Deserialize)]
            struct A {
                id: String,
                #[serde(default)]
                local_only: bool,
            }
            let a: A = args(a)?;
            ops::delete_item(app, &a.id, a.local_only)
        }
        "device_ls" => {
            #[derive(Deserialize)]
            struct A {
                #[serde(default)]
                path: String,
            }
            let a: A = args(a)?;
            ops::device_ls(app, &a.path)
        }
        "device_pull" => {
            #[derive(Deserialize)]
            struct A {
                path: String,
            }
            let a: A = args(a)?;
            Ok(json!(ops::start_pull(app, a.path)))
        }
        "device_mkdir" => {
            #[derive(Deserialize)]
            struct A {
                path: String,
            }
            let a: A = args(a)?;
            ops::device_mkdir(app, &a.path)?;
            Ok(Value::Null)
        }
        "device_delete" => {
            #[derive(Deserialize)]
            struct A {
                path: String,
            }
            let a: A = args(a)?;
            ops::device_delete(app, &a.path)
        }
        "ptpcamerad" => {
            #[derive(Deserialize)]
            struct A {
                disable: bool,
            }
            let a: A = args(a)?;
            ops::set_ptpcamerad(app, a.disable)?;
            Ok(Value::Null)
        }
        "copy_file" => {
            #[derive(Deserialize)]
            struct A {
                id: String,
                dest: PathBuf,
            }
            let a: A = args(a)?;
            let item = app.item(&a.id)?;
            std::fs::copy(app.item_path(&item), &a.dest).map_err(err)?;
            Ok(Value::Null)
        }
        "migrate" => {
            #[derive(Deserialize)]
            struct A {
                from: PathBuf,
            }
            let a: A = args(a)?;
            Ok(json!(ops::start_migrate(app, a.from)))
        }
        "reveal" => {
            #[derive(Deserialize)]
            struct A {
                id: Option<String>,
                path: Option<PathBuf>,
            }
            let a: A = args(a)?;
            let path = match (a.id, a.path) {
                (Some(id), _) => app.item_path(&app.item(&id)?),
                (None, Some(p)) => p,
                _ => return Err("nothing to show".into()),
            };
            std::process::Command::new("/usr/bin/open")
                .arg("-R")
                .arg(&path)
                .status()
                .map_err(err)?;
            Ok(Value::Null)
        }
        other => Err(format!("unknown command {other}")),
    }
}

/// A response for `folio://…` (Tauri) or `/folio/…` (dev bridge).
pub struct Fetched {
    pub mime: &'static str,
    pub body: Vec<u8>,
    /// Safe to cache in the web view (content-addressed).
    pub immutable: bool,
}

fn mime_of(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref() {
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("epub") => "application/epub+zip",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Paths: `notebook/<id>/page/<n>?w=<px>` (n from 0), `thumb/<id>`, `file/<id>`.
pub fn fetch(app: &Arc<App>, path: &str) -> std::result::Result<Fetched, (u16, String)> {
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let q = |k: &str| query.split('&').find_map(|kv| kv.strip_prefix(&format!("{k}=")));
    let not_found = |e: String| (404u16, e);
    match parts.as_slice() {
        ["notebook", id, "page", n] => {
            let page: u32 = n.parse().map_err(|_| (400, "bad page".to_string()))?;
            let width: u32 = q("w").and_then(|w| w.parse().ok()).unwrap_or(1024);
            let body = app.page_png(id, page, width).map_err(not_found)?;
            Ok(Fetched {
                mime: "image/png",
                body,
                immutable: false,
            })
        }
        ["thumb", id] => {
            let (mime, body) = app.thumbnail(id).map_err(not_found)?;
            Ok(Fetched {
                mime,
                body,
                immutable: false,
            })
        }
        ["annotation", id, "page", n] => {
            let page: usize = n.parse().map_err(|_| (400, "bad page".to_string()))?;
            let width: u32 = q("w").and_then(|w| w.parse().ok()).unwrap_or(1024);
            let body = crate::annotations::overlay_png(app, id, page, width).map_err(not_found)?;
            Ok(Fetched {
                mime: "image/png",
                body,
                immutable: false,
            })
        }
        ["file", id] => {
            let item = app.item(id).map_err(not_found)?;
            let body = std::fs::read(app.item_path(&item)).map_err(|e| (404, e.to_string()))?;
            Ok(Fetched {
                mime: mime_of(&item.rel_path),
                body,
                immutable: false,
            })
        }
        _ => Err((404, format!("unknown path {path}"))),
    }
}
