//! `folio-cli serve`: a development bridge that runs the app core behind HTTP so
//! the UI can be used and tested in an ordinary browser. The packaged app does
//! not use it (Tauri calls the same `api` directly).
//!
//! - `POST /api/<command>` with a JSON body → `{"ok": true, "value": …}` or `{"ok": false, "error": …}`
//! - `GET /folio/<path>` → page images and files (as `folio://` in the app)
//! - `GET /events` → server-sent events (as the app's `folio-event`)
//! - anything else → the built UI from `--ui` (single-page app)

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use folio_core::api;
use folio_core::app::{App, Event};
use folio_library::Layout;
use folio_library::cache::Cache;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use crate::Result;

type Clients = Arc<Mutex<Vec<Sender<String>>>>;

/// Same as `app/src-tauri/tauri.conf.json`. Scripts inside books are blocked
/// (`script-src 'self'`); their sections load as `blob:` frames, styles and fonts.
const APP_CSP: &str = "default-src 'self'; img-src 'self' folio: blob: data:; media-src 'self' blob: data:; connect-src 'self' folio: ipc: http://ipc.localhost; worker-src 'self' blob:; frame-src 'self' blob:; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline' blob:; font-src 'self' data: blob:";

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("valid header")
}

fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "json" => "application/json",
        "wasm" => "application/wasm",
        "ftl" => "text/plain",
        _ => "application/octet-stream",
    }
}

fn handle(app: &Arc<App>, clients: &Clients, ui: &Path, mut req: Request) {
    let url = req.url().to_string();
    let cors = header("Access-Control-Allow-Origin", "*");
    let result = if let (Method::Post, Some(name)) = (req.method(), url.strip_prefix("/api/")) {
        let mut body = String::new();
        let _ = req.as_reader().read_to_string(&mut body);
        let args: serde_json::Value = if body.trim().is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(&body).unwrap_or_default()
        };
        let args = if args.is_null() { serde_json::json!({}) } else { args };
        let reply = match api::call(app, name, args) {
            Ok(v) => serde_json::json!({"ok": true, "value": v}),
            Err(e) => serde_json::json!({"ok": false, "error": e}),
        };
        req.respond(
            Response::from_string(reply.to_string())
                .with_header(header("Content-Type", "application/json"))
                .with_header(cors),
        )
    } else if let Some(path) = url.strip_prefix("/folio/") {
        match api::fetch(app, path) {
            Ok(f) => req.respond(
                Response::from_data(f.body)
                    .with_header(header("Content-Type", f.mime))
                    .with_header(cors),
            ),
            Err((code, msg)) => req.respond(
                Response::from_string(msg)
                    .with_status_code(StatusCode(code))
                    .with_header(cors),
            ),
        }
    } else if url == "/events" {
        // Written by hand on the raw connection: tiny_http's chunk encoder
        // buffers, which would hold events back.
        let (tx, rx) = mpsc::channel::<String>();
        clients.lock().unwrap().push(tx);
        let mut w = req.into_writer();
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nAccess-Control-Allow-Origin: *\r\n\r\n: connected\n\n";
        if w.write_all(head.as_bytes()).and_then(|_| w.flush()).is_err() {
            return;
        }
        loop {
            let msg = match rx.recv_timeout(Duration::from_secs(15)) {
                Ok(json) => format!("data: {json}\n\n"),
                Err(RecvTimeoutError::Timeout) => ": keepalive\n\n".to_string(),
                Err(RecvTimeoutError::Disconnected) => return,
            };
            if w.write_all(msg.as_bytes()).and_then(|_| w.flush()).is_err() {
                return;
            }
        }
    } else {
        let rel = url.split('?').next().unwrap_or("/").trim_start_matches('/');
        let mut file = ui.join(if rel.is_empty() { "index.html" } else { rel });
        if !file.starts_with(ui) || !file.is_file() {
            file = ui.join("index.html"); // single-page app
        }
        match std::fs::read(&file) {
            Ok(data) => {
                let mut res = Response::from_data(data).with_header(header("Content-Type", mime(&file)));
                if file.extension().is_some_and(|e| e == "html") {
                    // the app's policy, so book iframes (foliate-js) are tested under it
                    res = res.with_header(header("Content-Security-Policy", APP_CSP));
                }
                req.respond(res)
            }
            Err(_) => req.respond(
                Response::from_string(format!(
                    "no UI build at {}; run `npm run build` in app/ui",
                    ui.display()
                ))
                .with_status_code(StatusCode(404)),
            ),
        }
    };
    let _ = result;
}

pub fn serve(port: u16, library: Option<PathBuf>, ui: Option<PathBuf>) -> Result<()> {
    let layout = Layout::new(library.unwrap_or_else(Layout::default_root));
    let ui = ui.unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/ui/dist"));
    let ui = ui.canonicalize().unwrap_or(ui);
    let clients: Clients = Arc::new(Mutex::new(Vec::new()));
    let sink_clients = clients.clone();
    let sink = Arc::new(move |e: &Event| {
        let json = serde_json::to_string(e).unwrap_or_default();
        sink_clients.lock().unwrap().retain(|c| c.send(json.clone()).is_ok());
    });
    let app = App::new(layout, Cache::new(Cache::default_root()), sink)?;
    app.start_device_watch();
    let server = Server::http(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    println!(
        "Folio dev bridge on http://127.0.0.1:{port}  (library {}, UI {})",
        app.layout.root.display(),
        ui.display()
    );
    for req in server.incoming_requests() {
        let (app, clients, ui) = (app.clone(), clients.clone(), ui.clone());
        std::thread::spawn(move || handle(&app, &clients, &ui, req));
    }
    Ok(())
}
