//! Folio, the macOS app: a Tauri 2 shell around `folio_core`.
//!
//! The UI (app/ui) reaches the core three ways, the same as through the dev
//! bridge: the `call` command (→ `folio_core::api::call`), the `folio://`
//! scheme for page images and files (→ `api::fetch`), and `folio-event`
//! events. Native menu choices are sent to the UI as `menu` events.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use folio_core::api;
use folio_core::app::{App, Event};
use folio_library::Layout;
use folio_library::cache::Cache;
use serde_json::Value;
use tauri::http::{Response, header};
use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, State, Wry};
use tauri_plugin_opener::OpenerExt;

const SOURCE_URL: &str = "https://github.com/azarezade/folio";

#[tauri::command]
async fn call(state: State<'_, Arc<App>>, name: String, args: Value) -> Result<Value, String> {
    let app = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api::call(&app, &name, args))
        .await
        .map_err(|e| e.to_string())?
}

/// The library folder: `FOLIO_LIBRARY` (or the older `SCRIBE_LIBRARY`) if
/// set, else `~/Library/Application Support/Folio`.
fn library_root() -> PathBuf {
    std::env::var_os("FOLIO_LIBRARY")
        .or_else(|| std::env::var_os("SCRIBE_LIBRARY"))
        .map(PathBuf::from)
        .unwrap_or_else(Layout::default_root)
}

fn menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let item = |id: &str, text: &str, accel: Option<&str>| MenuItem::with_id(app, id, text, true, accel);
    let about = AboutMetadata {
        name: Some("Folio".into()),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        copyright: Some("GPL-3.0-or-later".into()),
        comments: Some(
            "Reads, exports and syncs the notebooks, books and documents on a Kindle Scribe.\n\nFree software under the GNU GPL, version 3 or later; it comes with no warranty. Notebook and book decoding port John Howell's kfxlib (GPLv3).\n\nSource: github.com/azarezade/folio. Help → Licences lists the third-party components."
                .into(),
        ),
        website: Some(SOURCE_URL.into()),
        website_label: Some("Source code".into()),
        license: Some("GPL-3.0-or-later".into()),
        ..Default::default()
    };
    Menu::with_items(
        app,
        &[
            &Submenu::with_items(
                app,
                "Folio",
                true,
                &[
                    &PredefinedMenuItem::about(app, Some("About Folio"), Some(about))?,
                    &PredefinedMenuItem::separator(app)?,
                    &item("settings", "Settings…", Some("CmdOrCtrl+,"))?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::hide(app, None)?,
                    &PredefinedMenuItem::hide_others(app, None)?,
                    &PredefinedMenuItem::show_all(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::quit(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "File",
                true,
                &[
                    &item("export", "Export PDF…", Some("CmdOrCtrl+E"))?,
                    &item("push", "Push to Kindle…", Some("CmdOrCtrl+U"))?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::close_window(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "Edit",
                true,
                &[
                    &PredefinedMenuItem::undo(app, None)?,
                    &PredefinedMenuItem::redo(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::cut(app, None)?,
                    &PredefinedMenuItem::copy(app, None)?,
                    &PredefinedMenuItem::paste(app, None)?,
                    &PredefinedMenuItem::select_all(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &item("search_books", "Search in Books…", Some("CmdOrCtrl+Shift+F"))?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "View",
                true,
                &[
                    &item("book_mode", "Book Mode", Some("CmdOrCtrl+Shift+B"))?,
                    &item("zen", "Zen Mode", Some("CmdOrCtrl+Shift+Z"))?,
                    &PredefinedMenuItem::separator(app)?,
                    &item("zoom_in", "Zoom In", Some("CmdOrCtrl+="))?,
                    &item("zoom_out", "Zoom Out", Some("CmdOrCtrl+-"))?,
                    &item("zoom_reset", "Actual Size", Some("CmdOrCtrl+0"))?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::fullscreen(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "Device",
                true,
                &[
                    &item("sync_all", "Sync All", Some("CmdOrCtrl+Shift+S"))?,
                    &item("free", "Free the Kindle", None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &item("device", "Device & Transfers", Some("CmdOrCtrl+D"))?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "Window",
                true,
                &[
                    &PredefinedMenuItem::minimize(app, None)?,
                    &PredefinedMenuItem::maximize(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "Help",
                true,
                &[
                    &item("help_source", "Source Code", None)?,
                    &item("help_licences", "Licences", None)?,
                ],
            )?,
        ],
    )
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(|app| {
            let handle = app.handle().clone();
            let sink = Arc::new(move |e: &Event| {
                let _ = handle.emit("folio-event", e);
            });
            let core = App::new(Layout::new(library_root()), Cache::new(Cache::default_root()), sink)?;
            core.start_device_watch();
            app.manage(core);
            app.set_menu(menu(app.handle())?)?;
            app.on_menu_event(|app, event| match event.id().0.as_str() {
                "help_source" => {
                    let _ = app.opener().open_url(SOURCE_URL, None::<&str>);
                }
                "help_licences" => {
                    // bundled next to LICENSE in Contents/Resources
                    if let Ok(dir) = app.path().resource_dir() {
                        let _ = app
                            .opener()
                            .open_path(dir.join("THIRD_PARTY_NOTICES.txt").to_string_lossy(), None::<&str>);
                    }
                }
                id => {
                    let _ = app.emit("menu", id);
                }
            });
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("folio", |ctx, request, responder| {
            let app = ctx.app_handle().state::<Arc<App>>().inner().clone();
            let uri = request.uri();
            let path = match uri.query() {
                Some(q) => format!("{}?{q}", uri.path()),
                None => uri.path().to_string(),
            };
            std::thread::spawn(move || {
                let response = match api::fetch(&app, &path) {
                    Ok(f) => Response::builder()
                        .header(header::CONTENT_TYPE, f.mime)
                        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                        .body(f.body),
                    Err((code, msg)) => Response::builder()
                        .status(code)
                        .header(header::CONTENT_TYPE, "text/plain")
                        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                        .body(msg.into_bytes()),
                };
                responder.respond(response.expect("valid response"));
            });
        })
        .invoke_handler(tauri::generate_handler![call])
        .run(tauri::generate_context!())
        .expect("error while running Folio");
}
