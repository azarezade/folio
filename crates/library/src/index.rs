//! The library index (`index.sqlite` in the library root): one row per
//! notebook, book and screenshot, plus reading positions, local notebook
//! titles and settings.
//!
//! Item ids are a hash of the item's device path when it is known (from the
//! sync manifest, or `.notebooks/<uuid>/nbk` for notebooks), else of its path
//! in the library, so moving the library keeps ids. A file is re-hashed only
//! when its size or modification time changes; the content hash keys the
//! render cache, so edits invalidate cached pages.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::sync::Manifest;
use crate::{BookClass, Error, Layout, Result, classify_book};

const SCHEMA_VERSION: i64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Notebook,
    Book,
    Screenshot,
}

impl ItemKind {
    fn as_str(self) -> &'static str {
        match self {
            ItemKind::Notebook => "notebook",
            ItemKind::Book => "book",
            ItemKind::Screenshot => "screenshot",
        }
    }

    fn parse(s: &str) -> ItemKind {
        match s {
            "notebook" => ItemKind::Notebook,
            "book" => ItemKind::Book,
            _ => ItemKind::Screenshot,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Item {
    pub id: String,
    pub kind: ItemKind,
    /// Path relative to the library root.
    pub rel_path: String,
    pub device_path: Option<String>,
    pub title: String,
    pub authors: Vec<String>,
    /// Books: purchased or sideloaded.
    pub class: Option<BookClass>,
    /// Display format: "Notebook", "PDF", "EPUB", "KFX", …
    pub format: String,
    pub size: u64,
    /// Unix seconds (screenshots: when it was taken, from the file name).
    pub modified: i64,
    pub content_hash: String,
    pub page_count: Option<u32>,
    /// Library-relative path of the device's thumbnail (notebooks).
    pub thumbnail: Option<String>,
    /// Books: why it can't be opened here (DRM, unsupported format).
    pub blocked: Option<String>,
    /// Books: has a cover image (served as its thumbnail).
    pub has_cover: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct TextHit {
    pub id: String,
    pub loc: String,
    pub label: String,
    pub snippet: String,
}

pub struct Index {
    conn: Connection,
    layout: Layout,
}

fn hash_id(key: &str) -> String {
    blake3::hash(key.as_bytes()).to_hex()[..12].to_string()
}

fn file_hash(path: &Path) -> std::io::Result<String> {
    let mut h = blake3::Hasher::new();
    h.update_reader(std::fs::File::open(path)?)?;
    Ok(h.finalize().to_hex()[..32].to_string())
}

fn mtime(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

const BOOK_EXTENSIONS: [&str; 11] = [
    "pdf", "epub", "kfx", "azw", "azw3", "azw8", "mobi", "prc", "txt", "kfx-zip", "docx",
];

fn format_name(ext: &str) -> String {
    match ext {
        "pdf" => "PDF".into(),
        "epub" => "EPUB".into(),
        "kfx" | "kfx-zip" => "KFX".into(),
        "azw3" => "AZW3".into(),
        "azw" | "azw8" => "AZW".into(),
        "mobi" | "prc" => "MOBI".into(),
        "txt" => "Text".into(),
        other => other.to_ascii_uppercase(),
    }
}

/// `library.py:_filename_to_title`: underscores and dashes become spaces, and
/// all-lowercase names are title-cased; ASINs and hex hashes are kept.
pub fn title_from_filename(stem: &str) -> String {
    let is_asin = stem.len() == 10 && stem.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    let is_hex = stem.len() == 32 && stem.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
    if is_asin || is_hex {
        return stem.to_string();
    }
    let pretty = stem
        .split(['_', '-'])
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let pretty = pretty.split_whitespace().collect::<Vec<_>>().join(" ");
    if pretty.chars().any(|c| c.is_alphabetic()) && !pretty.chars().any(|c| c.is_uppercase()) {
        pretty
            .split(' ')
            .map(|w| {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect::<String>())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        pretty
    }
}

/// `Screenshot_2024-04-29-10-30-45…` → Unix seconds (local time on the device;
/// stored as if UTC, which is fine for sorting and display).
pub fn screenshot_time(name: &str) -> Option<i64> {
    let rest = name.strip_prefix("Screenshot_")?;
    let digits: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '_')
        .filter(char::is_ascii_digit)
        .collect();
    if digits.len() < 14 {
        return None;
    }
    let n = |a: usize, b: usize| digits[a..b].parse::<i64>().ok();
    let (y, mo, d, h, mi, s) = (n(0, 4)?, n(4, 6)?, n(6, 8)?, n(8, 10)?, n(10, 12)?, n(12, 14)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    // days from civil (Howard Hinnant)
    let (y2, m2) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + s)
}

impl Index {
    pub fn open(layout: &Layout) -> Result<Index> {
        std::fs::create_dir_all(&layout.root)?;
        let conn = Connection::open(layout.root.join("index.sqlite")).map_err(sql)?;
        conn.pragma_update(None, "journal_mode", "WAL").map_err(sql)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(sql)?;
        if version != SCHEMA_VERSION {
            conn.execute_batch(
                "DROP TABLE IF EXISTS items;
                 CREATE TABLE items(
                    id TEXT PRIMARY KEY, kind TEXT NOT NULL, rel_path TEXT NOT NULL UNIQUE, device_path TEXT,
                    title TEXT NOT NULL, authors TEXT NOT NULL DEFAULT '[]', class TEXT, format TEXT NOT NULL,
                    size INTEGER NOT NULL, mtime INTEGER NOT NULL, mtime_ns INTEGER NOT NULL, modified INTEGER NOT NULL,
                    content_hash TEXT NOT NULL, page_count INTEGER, thumbnail TEXT,
                    blocked TEXT, has_cover INTEGER NOT NULL DEFAULT 0);
                 CREATE TABLE IF NOT EXISTS titles(id TEXT PRIMARY KEY, title TEXT NOT NULL);
                 CREATE TABLE IF NOT EXISTS positions(id TEXT PRIMARY KEY, page INTEGER NOT NULL, updated INTEGER NOT NULL);
                 CREATE TABLE IF NOT EXISTS locations(id TEXT PRIMARY KEY, location TEXT NOT NULL, fraction REAL, updated INTEGER NOT NULL);
                 CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);",
            )
            .map_err(sql)?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION).map_err(sql)?;
        }
        // Full-text search over book text (Phase 8), filled in the background.
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(
                text, id UNINDEXED, loc UNINDEXED, label UNINDEXED, tokenize = 'unicode61 remove_diacritics 2');
             CREATE TABLE IF NOT EXISTS fts_state(id TEXT PRIMARY KEY, content_hash TEXT NOT NULL, chunks INTEGER NOT NULL);",
        )
        .map_err(sql)?;
        Ok(Index {
            conn,
            layout: layout.clone(),
        })
    }

    /// Brings the index up to date with the files in the library. Returns the
    /// number of items added or changed. `page_count` is asked for notebooks
    /// whose content changed.
    pub fn scan(&mut self, page_count: &dyn Fn(&Path) -> Option<u32>) -> Result<usize> {
        let manifest = Manifest::load(&self.layout);
        let device_paths: HashMap<&str, &str> = manifest
            .files
            .iter()
            .map(|(local, e)| (local.as_str(), e.device_path.as_str()))
            .collect();

        let mut found: Vec<(ItemKind, PathBuf)> = Vec::new();
        let root = &self.layout.root;
        let list = |dir: &Path| -> Vec<PathBuf> {
            let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
                .map(|rd| rd.filter_map(|e| Some(e.ok()?.path())).collect())
                .unwrap_or_default();
            v.sort();
            v
        };
        for p in list(&root.join("notebooks")) {
            if p.extension().is_some_and(|e| e == "nbk") {
                found.push((ItemKind::Notebook, p));
            }
        }
        for class in ["purchased", "sideloaded"] {
            let mut stack = vec![root.join("books").join(class)];
            while let Some(dir) = stack.pop() {
                for p in list(&dir) {
                    let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                    if name.starts_with('.') || name.ends_with(".part") {
                        continue;
                    }
                    if p.is_dir() {
                        if !name.to_ascii_lowercase().ends_with(".sdr") {
                            stack.push(p);
                        }
                    } else if BOOK_EXTENSIONS.contains(&ext_of(&name).as_str()) {
                        found.push((ItemKind::Book, p));
                    }
                }
            }
        }
        for p in list(&root.join("screenshots")) {
            let e = ext_of(&p.to_string_lossy());
            if ["png", "jpg", "jpeg"].contains(&e.as_str()) {
                found.push((ItemKind::Screenshot, p));
            }
        }

        let existing: HashMap<String, (i64, i64, String, Option<u32>)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT rel_path, size, mtime_ns, content_hash, page_count FROM items")
                .map_err(sql)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
                })
                .map_err(sql)?;
            rows.collect::<rusqlite::Result<_>>().map_err(sql)?
        };

        let tx = self.conn.transaction().map_err(sql)?;
        let mut seen = std::collections::HashSet::new();
        let mut changed = 0;
        for (kind, path) in found {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            let size = meta.len() as i64;
            let mtime_ns = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos() as i64);
            seen.insert(rel.clone());
            let prior = existing.get(&rel);
            if prior.is_some_and(|p| p.0 == size && p.1 == mtime_ns) {
                continue;
            }
            changed += 1;
            let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s).to_string();
            let content_hash = file_hash(&path)?;
            let device_path = match kind {
                ItemKind::Notebook => device_paths
                    .get(rel.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| Some(format!(".notebooks/{stem}/nbk"))),
                _ => device_paths.get(rel.as_str()).map(|s| s.to_string()),
            };
            let id = hash_id(
                device_path
                    .as_deref()
                    .map_or(format!("local:{rel}"), |d| format!("device:{d}"))
                    .as_str(),
            );
            let mut book = folio_kfx::BookInfo::default();
            let (title, class, format, page_count, thumbnail, modified) = match kind {
                ItemKind::Notebook => {
                    let pc = if prior.is_some_and(|p| p.2 == content_hash) {
                        prior.and_then(|p| p.3)
                    } else {
                        page_count(&path)
                    };
                    let thumb = format!("notebooks/{stem}.png");
                    let thumb = root.join(&thumb).exists().then_some(thumb);
                    (stem.clone(), None, "Notebook".to_string(), pc, thumb, mtime(&meta))
                }
                ItemKind::Book => {
                    let parent = path
                        .parent()
                        .and_then(|p| p.file_name())
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let class = if rel.starts_with("books/purchased/") {
                        BookClass::Purchased
                    } else {
                        classify_book(&stem, &parent)
                    };
                    let (book_kind, info) = folio_kfx::inspect(&path);
                    let format = match book_kind {
                        folio_kfx::Kind::Kf8 => "AZW3".to_string(),
                        folio_kfx::Kind::Kfx => "KFX".to_string(),
                        _ => format_name(&ext_of(&name)),
                    };
                    let title = info.title.clone().unwrap_or_else(|| title_from_filename(&stem));
                    book = info;
                    (title, Some(class), format, None, None, mtime(&meta))
                }
                ItemKind::Screenshot => {
                    let t = screenshot_time(&name).unwrap_or_else(|| mtime(&meta));
                    (name.clone(), None, ext_of(&name).to_ascii_uppercase(), None, None, t)
                }
            };
            tx.execute(
                "INSERT INTO items(id, kind, rel_path, device_path, title, authors, class, format, size, mtime, mtime_ns, modified, content_hash, page_count, thumbnail, blocked, has_cover)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?15, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?16, ?17)
                 ON CONFLICT(rel_path) DO UPDATE SET id=?1, kind=?2, device_path=?4, title=?5, class=?6, format=?7, size=?8, mtime=?9,
                    mtime_ns=?10, modified=?11, content_hash=?12, page_count=?13, thumbnail=?14, authors=?15, blocked=?16, has_cover=?17",
                params![
                    id,
                    kind.as_str(),
                    rel,
                    device_path,
                    title,
                    class.map(|c| match c {
                        BookClass::Purchased => "purchased",
                        BookClass::Sideloaded => "sideloaded",
                    }),
                    format,
                    size,
                    mtime(&meta),
                    mtime_ns,
                    modified,
                    content_hash,
                    page_count,
                    thumbnail,
                    serde_json::to_string(&book.authors).unwrap_or_default(),
                    book.blocked,
                    book.has_cover
                ],
            )
            .map_err(sql)?;
        }
        for rel in existing.keys().filter(|r| !seen.contains(*r)) {
            tx.execute(
                "DELETE FROM fts WHERE id IN (SELECT id FROM items WHERE rel_path = ?1)",
                [rel],
            )
            .map_err(sql)?;
            tx.execute(
                "DELETE FROM fts_state WHERE id IN (SELECT id FROM items WHERE rel_path = ?1)",
                [rel],
            )
            .map_err(sql)?;
            tx.execute("DELETE FROM items WHERE rel_path = ?1", [rel])
                .map_err(sql)?;
            changed += 1;
        }
        tx.commit().map_err(sql)?;
        Ok(changed)
    }

    fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Item> {
        let class: Option<String> = r.get("class")?;
        let authors: String = r.get("authors")?;
        Ok(Item {
            id: r.get("id")?,
            kind: ItemKind::parse(&r.get::<_, String>("kind")?),
            rel_path: r.get("rel_path")?,
            device_path: r.get("device_path")?,
            title: r.get::<_, Option<String>>("custom_title")?.unwrap_or(r.get("title")?),
            authors: serde_json::from_str(&authors).unwrap_or_default(),
            class: class.map(|c| {
                if c == "purchased" {
                    BookClass::Purchased
                } else {
                    BookClass::Sideloaded
                }
            }),
            format: r.get("format")?,
            size: r.get::<_, i64>("size")? as u64,
            modified: r.get("modified")?,
            content_hash: r.get("content_hash")?,
            page_count: r.get("page_count")?,
            thumbnail: r.get("thumbnail")?,
            blocked: r.get("blocked")?,
            has_cover: r.get("has_cover")?,
        })
    }

    const SELECT: &'static str =
        "SELECT items.*, titles.title AS custom_title FROM items LEFT JOIN titles ON titles.id = items.id";

    /// Items of a kind, newest first.
    pub fn items(&self, kind: Option<ItemKind>) -> Result<Vec<Item>> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "{} WHERE (?1 IS NULL OR kind = ?1) ORDER BY modified DESC, rel_path",
                Self::SELECT
            ))
            .map_err(sql)?;
        let rows = stmt.query_map([kind.map(ItemKind::as_str)], Self::row).map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    pub fn get(&self, id: &str) -> Result<Option<Item>> {
        self.conn
            .query_row(&format!("{} WHERE items.id = ?1", Self::SELECT), [id], Self::row)
            .optional()
            .map_err(sql)
    }

    pub fn path(&self, item: &Item) -> PathBuf {
        self.layout.root.join(&item.rel_path)
    }

    /// A local title for a notebook (the Kindle's titles are not readable yet).
    pub fn rename(&self, id: &str, title: Option<&str>) -> Result<()> {
        match title.map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) => self.conn.execute(
                "INSERT OR REPLACE INTO titles(id, title) VALUES(?1, ?2)",
                params![id, t],
            ),
            None => self.conn.execute("DELETE FROM titles WHERE id = ?1", [id]),
        }
        .map_err(sql)?;
        Ok(())
    }

    pub fn position(&self, id: &str) -> Result<Option<u32>> {
        self.conn
            .query_row("SELECT page FROM positions WHERE id = ?1", [id], |r| r.get(0))
            .optional()
            .map_err(sql)
    }

    pub fn set_position(&self, id: &str, page: u32) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        self.conn
            .execute(
                "INSERT OR REPLACE INTO positions(id, page, updated) VALUES(?1, ?2, ?3)",
                params![id, page, now],
            )
            .map_err(sql)?;
        Ok(())
    }

    /// A reflowable book's reading location (an EPUB CFI) and how far in it is.
    pub fn location(&self, id: &str) -> Result<Option<(String, Option<f64>)>> {
        self.conn
            .query_row("SELECT location, fraction FROM locations WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()
            .map_err(sql)
    }

    pub fn set_location(&self, id: &str, location: &str, fraction: Option<f64>) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        self.conn
            .execute(
                "INSERT OR REPLACE INTO locations(id, location, fraction, updated) VALUES(?1, ?2, ?3, ?4)",
                params![id, location, fraction, now],
            )
            .map_err(sql)?;
        Ok(())
    }

    /// Books whose text isn't indexed for search, or changed since.
    pub fn unindexed_books(&self) -> Result<Vec<Item>> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "{} LEFT JOIN fts_state s ON s.id = items.id
                 WHERE items.kind = 'book' AND items.blocked IS NULL
                   AND (s.content_hash IS NULL OR s.content_hash != items.content_hash)
                 ORDER BY items.size",
                Self::SELECT
            ))
            .map_err(sql)?;
        let rows = stmt.query_map([], Self::row).map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    /// Replaces a book's searchable text. `chunks` are (location, label, text).
    pub fn set_text(&mut self, id: &str, content_hash: &str, chunks: &[(String, String, String)]) -> Result<()> {
        let tx = self.conn.transaction().map_err(sql)?;
        tx.execute("DELETE FROM fts WHERE id = ?1", [id]).map_err(sql)?;
        for (loc, label, text) in chunks {
            tx.execute(
                "INSERT INTO fts(text, id, loc, label) VALUES(?1, ?2, ?3, ?4)",
                params![text, id, loc, label],
            )
            .map_err(sql)?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO fts_state(id, content_hash, chunks) VALUES(?1, ?2, ?3)",
            params![id, content_hash, chunks.len() as i64],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)
    }

    /// Searches book text. Every word must match; the last may be a prefix.
    /// Returns the best chunks, at most `limit`, with a highlighted snippet
    /// (matches between \u{2} and \u{3}).
    pub fn search_text(&self, query: &str, limit: usize) -> Result<Vec<TextHit>> {
        let words: Vec<String> = query
            .split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’')
            .filter(|w| !w.is_empty())
            .map(|w| format!("\"{}\"", w.replace('"', "")))
            .collect();
        let Some(last) = words.last() else {
            return Ok(Vec::new());
        };
        let fts_query = format!("{} {last}*", words[..words.len() - 1].join(" "));
        let mut stmt = self
            .conn
            .prepare(
                "SELECT fts.id, fts.loc, fts.label, snippet(fts, 0, char(2), char(3), '…', 16)
                 FROM fts JOIN items ON items.id = fts.id
                 WHERE fts MATCH ?1 ORDER BY rank LIMIT ?2",
            )
            .map_err(sql)?;
        let rows = stmt
            .query_map(params![fts_query.trim(), limit as i64], |r| {
                Ok(TextHit {
                    id: r.get(0)?,
                    loc: r.get(1)?,
                    label: r.get(2)?,
                    snippet: r.get(3)?,
                })
            })
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    /// (books indexed, chunks) for the status line.
    pub fn text_stats(&self) -> Result<(i64, i64)> {
        self.conn
            .query_row("SELECT count(*), coalesce(sum(chunks), 0) FROM fts_state", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(sql)
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
            .optional()
            .map_err(sql)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO settings(key, value) VALUES(?1, ?2)",
                params![key, value],
            )
            .map_err(sql)?;
        Ok(())
    }

    pub fn counts(&self) -> Result<HashMap<ItemKind, usize>> {
        let mut stmt = self
            .conn
            .prepare("SELECT kind, count(*) FROM items GROUP BY kind")
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((ItemKind::parse(&r.get::<_, String>(0)?), r.get::<_, i64>(1)? as usize))
            })
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }
}

fn ext_of(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

fn sql(e: rusqlite::Error) -> Error {
    Error::Other(format!("index database: {e}"))
}

/// Reads at most `n` bytes (e.g. a file header).
pub fn read_head(path: &Path, n: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(n);
    std::fs::File::open(path)?.take(n as u64).read_to_end(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles() {
        assert_eq!(
            title_from_filename("Oxford_Dictionary_of_English"),
            "Oxford Dictionary of English"
        );
        assert_eq!(title_from_filename("my-great_book"), "My Great Book");
        assert_eq!(title_from_filename("B07XYZ1234"), "B07XYZ1234");
        assert_eq!(title_from_filename("My Clippings"), "My Clippings");
    }

    #[test]
    fn screenshot_times() {
        assert_eq!(screenshot_time("Screenshot_2024-04-29-10-30-45.png"), Some(1714386645));
        assert_eq!(screenshot_time("Screenshot_20240429_103045.png"), Some(1714386645));
        assert_eq!(screenshot_time("photo.png"), None);
    }

    #[test]
    fn scan_and_ids() {
        let root = std::env::temp_dir().join(format!("folio-index-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let layout = Layout::new(&root);
        std::fs::create_dir_all(root.join("notebooks")).unwrap();
        std::fs::create_dir_all(root.join("books/sideloaded/Papers/x.sdr")).unwrap();
        std::fs::create_dir_all(root.join("screenshots")).unwrap();
        std::fs::write(root.join("notebooks/0da54486-3c07-10a2-0c3c-f2c846c34b46.nbk"), b"nbk").unwrap();
        std::fs::write(root.join("books/sideloaded/Papers/paper_one.pdf"), b"%PDF").unwrap();
        std::fs::write(root.join("books/sideloaded/Papers/x.sdr/x.pdf"), b"sidecar").unwrap();
        std::fs::write(root.join("screenshots/Screenshot_2024-04-29-10-30-45.png"), b"png").unwrap();
        let mut idx = Index::open(&layout).unwrap();
        assert_eq!(idx.scan(&|_| Some(3)).unwrap(), 3);
        assert_eq!(idx.scan(&|_| Some(3)).unwrap(), 0, "unchanged files are not rescanned");
        let items = idx.items(None).unwrap();
        assert_eq!(items.len(), 3, "sidecars are not items");
        let nb = items.iter().find(|i| i.kind == ItemKind::Notebook).unwrap();
        assert_eq!(nb.page_count, Some(3));
        assert_eq!(
            nb.id,
            hash_id("device:.notebooks/0da54486-3c07-10a2-0c3c-f2c846c34b46/nbk")
        );
        let book = items.iter().find(|i| i.kind == ItemKind::Book).unwrap();
        assert_eq!(book.title, "Paper One");
        idx.rename(&nb.id, Some("Portraits")).unwrap();
        assert_eq!(idx.get(&nb.id).unwrap().unwrap().title, "Portraits");
        idx.set_position(&book.id, 7).unwrap();
        assert_eq!(idx.position(&book.id).unwrap(), Some(7));
        assert_eq!(idx.unindexed_books().unwrap().len(), 1);
        let chunks = [
            (
                "page:0".into(),
                "p. 1".into(),
                "Portraits of the lighthouse keeper".into(),
            ),
            ("page:1".into(), "p. 2".into(), "Nothing to see — café au lait".into()),
        ];
        idx.set_text(&book.id, &book.content_hash, &chunks).unwrap();
        assert!(idx.unindexed_books().unwrap().is_empty());
        let hits = idx.search_text("lighthouse kee", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            (hits[0].loc.as_str(), hits[0].id.as_str()),
            ("page:0", book.id.as_str())
        );
        assert!(hits[0].snippet.contains("\u{2}lighthouse\u{3}"), "{}", hits[0].snippet);
        assert_eq!(idx.search_text("cafe", 10).unwrap().len(), 1, "diacritics folded");
        assert!(
            idx.search_text("\"* OR (", 10).unwrap().is_empty(),
            "query syntax is escaped"
        );
        std::fs::remove_file(root.join("screenshots/Screenshot_2024-04-29-10-30-45.png")).unwrap();
        assert_eq!(idx.scan(&|_| None).unwrap(), 1);
        assert_eq!(idx.items(Some(ItemKind::Screenshot)).unwrap().len(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }
}
