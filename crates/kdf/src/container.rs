//! A KDF (SQLite) container opened as a lazy fragment store.
//!
//! Port of `kfxlib/kpf_container.py` (`KpfContainer.deserialize`,
//! `process_db_fragment`). Only ids, types and blob lengths are read up front;
//! payloads are fetched from SQLite when asked for.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use folio_ion::symtab::sys;
use folio_ion::write::{Owned, to_document};
use folio_ion::{self as ion, Diagnostics, SIGNATURE, SymbolId, SymbolTable, Value, peek_annotations, read_single};
use rusqlite::{Connection, MAIN_DB, OpenFlags};

use crate::fingerprint;
use crate::sym;
use crate::{Error, Result};

pub const DRMION_SIGNATURE: &[u8] = b"\xeaDRMION\xee";
const SQLITE_SIGNATURE: &[u8] = b"SQLite format 3\0";
/// Bytes read from each payload to find its type annotation.
const HEAD_LEN: usize = 64;

/// Table schemas kfxlib accepts, compared by exact SQL text (`kpf_container.py:40`).
mod schema {
    pub const INDEX_INFO: &str = "CREATE TABLE index_info(namespace char(256), index_name char(256), property char(40), primary key (namespace, index_name)) without rowid";
    pub const KFXID_TRANSLATION: &str =
        "CREATE TABLE kfxid_translation(eid INTEGER, kfxid char(40), primary key(eid)) without rowid";
    pub const FRAGMENT_PROPERTIES: &str = "CREATE TABLE fragment_properties(id char(40), key char(40), value char(40), primary key (id, key, value)) without rowid";
    pub const ACTION_FRAGMENTS: &str = "CREATE TABLE local_action_fragments(action_id char(40), payload_type char(10), payload_value blob, primary key (action_id))";
    pub const DELTA_FRAGMENTS: &str = "CREATE TABLE local_delta_fragments(id char(40), payload_type char(10), payload_value blob, deleted smallint, primary key (id))";
    pub const FRAGMENTS: &str =
        "CREATE TABLE fragments(id char(40), payload_type char(10), payload_value blob, primary key (id))";
    pub const GC_FRAGMENT_PROPERTIES: &str = "CREATE TABLE gc_fragment_properties(id varchar(40), key varchar(40), value varchar(40), primary key (id, key, value)) without rowid";
    pub const GC_REACHABLE: &str = "CREATE TABLE gc_reachable(id varchar(40), primary key (id)) without rowid";
    pub const CAPABILITIES: &str =
        "CREATE TABLE capabilities(key char(20), version smallint, primary key (key, version)) without rowid";
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Table {
    Fragments,
    Delta,
}

impl Table {
    fn name(self) -> &'static str {
        match self {
            Table::Fragments => "fragments",
            Table::Delta => "local_delta_fragments",
        }
    }
}

/// A fragment's identity, as kfxlib keys it: root fragment types have no id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FragmentKey {
    pub ftype: SymbolId,
    pub fid: Option<Box<str>>,
}

impl FragmentKey {
    pub fn new(ftype: SymbolId, fid: Option<&str>) -> Self {
        FragmentKey {
            ftype,
            fid: fid.map(Into::into),
        }
    }
}

#[derive(Clone, Debug)]
enum Payload {
    Row {
        table: Table,
        rowid: i64,
    },
    /// A value kfxlib synthesises, as an Ion document.
    Inline(Vec<u8>),
}

#[derive(Clone, Debug)]
pub struct Fragment {
    pub key: FragmentKey,
    /// The row id in the database (empty for synthetic fragments).
    pub db_id: Box<str>,
    /// A raw resource blob (`$417`) rather than an Ion value.
    pub raw: bool,
    /// Stored payload size in bytes.
    pub len: u64,
    payload: Payload,
}

impl Fragment {
    pub fn ftype(&self) -> SymbolId {
        self.key.ftype
    }

    pub fn fid(&self) -> Option<&str> {
        self.key.fid.as_deref()
    }
}

/// A fetched payload.
pub struct FragmentData {
    bytes: Vec<u8>,
    raw: bool,
}

impl FragmentData {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The fragment's value: the Ion value under its type annotation, or the raw blob.
    pub fn value(&self) -> ion::Result<Value<'_>> {
        if self.raw {
            return Ok(Value::Blob(&self.bytes));
        }
        match read_single(&self.bytes)? {
            Value::Annotated(a) => a.value(),
            v => Ok(v),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct OpenOptions {
    /// Where fingerprint-free copies of fingerprinted files are kept, keyed by
    /// content hash, so later opens skip the rewrite. Defaults to a folder in
    /// the system temp directory.
    pub cache_dir: Option<PathBuf>,
}

pub struct Kdf {
    conn: Connection,
    db_path: PathBuf,
    symtab: SymbolTable,
    fragments: Vec<Fragment>,
    index: HashMap<FragmentKey, usize>,
    eids: HashMap<i64, Box<str>>,
    is_notebook: bool,
    is_dictionary: bool,
    fingerprints_removed: usize,
    diag: Diagnostics,
}

impl Kdf {
    pub fn open(path: impl AsRef<Path>, options: &OpenOptions) -> Result<Kdf> {
        let path = path.as_ref();
        let mut diag = Diagnostics::default();
        let (db_path, fingerprints_removed) = unwrapped_database(path, options, &mut diag)?;

        let conn = connect(&db_path)?;

        let mut kdf = Kdf {
            conn,
            db_path,
            symtab: SymbolTable::with_yj_symbols(ion::symtab::YJ_SYMBOLS_LEN, &mut diag),
            fragments: Vec::new(),
            index: HashMap::new(),
            eids: HashMap::new(),
            is_notebook: false,
            is_dictionary: false,
            fingerprints_removed,
            diag: Diagnostics::default(),
        };
        kdf.load(&mut diag)?;
        kdf.diag = diag;
        Ok(kdf)
    }

    /// Another handle on the same database, with its own SQLite connection (for
    /// use on another thread). The fragment index is copied, not rebuilt.
    pub fn try_clone(&self) -> Result<Kdf> {
        Ok(Kdf {
            conn: connect(&self.db_path)?,
            db_path: self.db_path.clone(),
            symtab: self.symtab.clone(),
            fragments: self.fragments.clone(),
            index: self.index.clone(),
            eids: self.eids.clone(),
            is_notebook: self.is_notebook,
            is_dictionary: self.is_dictionary,
            fingerprints_removed: self.fingerprints_removed,
            diag: self.diag.clone(),
        })
    }

    /// The SQLite file actually opened (the cached copy for fingerprinted files).
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn fingerprints_removed(&self) -> usize {
        self.fingerprints_removed
    }

    pub fn symtab(&self) -> &SymbolTable {
        &self.symtab
    }

    /// Has the notebook-only tables (`local_action_fragments` or `local_delta_fragments`).
    pub fn is_notebook(&self) -> bool {
        self.is_notebook
    }

    pub fn is_dictionary(&self) -> bool {
        self.is_dictionary
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diag
    }

    pub fn fragments(&self) -> &[Fragment] {
        &self.fragments
    }

    pub fn get(&self, ftype: SymbolId, fid: Option<&str>) -> Option<&Fragment> {
        self.index
            .get(&FragmentKey::new(ftype, fid))
            .map(|&i| &self.fragments[i])
    }

    pub fn get_key(&self, key: &FragmentKey) -> Option<&Fragment> {
        self.index.get(key).map(|&i| &self.fragments[i])
    }

    /// The first fragment of a type (for root types, the only one).
    pub fn first_of(&self, ftype: SymbolId) -> Option<&Fragment> {
        self.fragments.iter().find(|f| f.ftype() == ftype)
    }

    pub fn read(&self, fragment: &Fragment) -> Result<FragmentData> {
        let bytes = match &fragment.payload {
            Payload::Inline(b) => b.clone(),
            Payload::Row { table, rowid } => self.read_blob(*table, *rowid, None)?,
        };
        Ok(FragmentData {
            bytes,
            raw: fragment.raw,
        })
    }

    /// The name a reference value points at: a symbol, a `$598::"kfxid"` string,
    /// or a `$598::eid` integer (resolved through `kfxid_translation`).
    pub fn ref_name<'v>(&'v self, value: Value<'v>) -> Option<std::borrow::Cow<'v, str>> {
        match value {
            Value::Symbol(id) => Some(self.symtab.name(id)),
            Value::Annotated(a) if a.first_annotation() == Ok(sym::KFX_ID) => match a.value().ok()? {
                Value::String(s) => Some(s.into()),
                Value::Int(eid) => self.eid(i64::try_from(eid).ok()?).map(Into::into),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn eid(&self, eid: i64) -> Option<&str> {
        self.eids.get(&eid).map(|s| &**s)
    }

    fn read_blob(&self, table: Table, rowid: i64, limit: Option<usize>) -> Result<Vec<u8>> {
        let mut blob = self
            .conn
            .blob_open(MAIN_DB, table.name(), "payload_value", rowid, true)?;
        let len = blob.len();
        let mut buf = vec![0; limit.map_or(len, |l| l.min(len))];
        blob.read_exact(&mut buf)?;
        Ok(buf)
    }

    fn load(&mut self, diag: &mut Diagnostics) -> Result<()> {
        let mut tables: HashSet<String> = self
            .conn
            .prepare("SELECT sql FROM sqlite_master WHERE type='table'")?
            .query_map([], |r| r.get::<_, Option<String>>(0))?
            .filter_map(|r| r.ok().flatten())
            .collect();
        let mut has = |sql: &str| tables.remove(sql);

        if has(schema::INDEX_INFO) {
            self.is_dictionary = true;
            diag.warning("kdf.dictionary", || "dictionary index tables are not read".into());
        }

        if has(schema::KFXID_TRANSLATION) {
            let mut stmt = self.conn.prepare("SELECT eid, kfxid FROM kfxid_translation")?;
            for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
                let (eid, kfxid) = row?;
                self.eids.insert(eid, kfxid.into());
            }
        }

        let mut element_types = HashMap::new();
        if has(schema::FRAGMENT_PROPERTIES) {
            let mut stmt = self.conn.prepare("SELECT id, key, value FROM fragment_properties")?;
            for row in stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })? {
                let (id, key, value) = row?;
                match key.as_str() {
                    "child" => {}
                    "element_type" => {
                        element_types.insert(id, value);
                    }
                    _ => diag.error("kdf.fragment_property", || {
                        format!("unknown fragment_property {id} {key}={value}")
                    }),
                }
            }
        }

        if has(schema::ACTION_FRAGMENTS) {
            self.is_notebook = true;
            let n: i64 = self
                .conn
                .query_row("SELECT count(*) FROM local_action_fragments", [], |r| r.get(0))?;
            for _ in 0..n {
                diag.debug("kdf.action", || {
                    "local_action_fragments rows (device edit log) are not used".into()
                });
            }
        }

        let has_delta = has(schema::DELTA_FRAGMENTS);
        if has_delta {
            self.is_notebook = true;
        }

        if has(schema::FRAGMENTS) {
            let mut have_symtab = false;
            for condition in [" WHERE id = '$ion_symbol_table'", " WHERE id = 'max_id'", ""] {
                let mut overridden = HashSet::new();
                let mut rows = Vec::new();
                if has_delta {
                    let mut stmt = self.conn.prepare(&format!(
                        "SELECT rowid, id, payload_type, length(payload_value), deleted FROM local_delta_fragments{condition}"
                    ))?;
                    for row in stmt.query_map([], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                            r.get::<_, Option<i64>>(3)?,
                            r.get::<_, Option<i64>>(4)?,
                        ))
                    })? {
                        let (rowid, id, ptype, len, deleted) = row?;
                        overridden.insert(id.clone());
                        if deleted.unwrap_or(0) == 0 {
                            rows.push((Table::Delta, rowid, id, ptype, len.unwrap_or(0)));
                        }
                    }
                }
                let mut stmt = self.conn.prepare(&format!(
                    "SELECT rowid, id, payload_type, length(payload_value) FROM fragments{condition}"
                ))?;
                for row in stmt.query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        r.get::<_, Option<i64>>(3)?,
                    ))
                })? {
                    let (rowid, id, ptype, len) = row?;
                    if !overridden.contains(&id) {
                        rows.push((Table::Fragments, rowid, id, ptype, len.unwrap_or(0)));
                    }
                }
                drop(stmt);
                for (table, rowid, id, ptype, len) in rows {
                    self.process_row(
                        table,
                        rowid,
                        &id,
                        &ptype,
                        len as u64,
                        &element_types,
                        &mut have_symtab,
                        diag,
                    )?;
                }
            }
        } else {
            diag.error("kdf.no_fragments", || {
                "KDF database is missing the 'fragments' table".into()
            });
        }

        has(schema::GC_FRAGMENT_PROPERTIES);
        has(schema::GC_REACHABLE);

        if has(schema::CAPABILITIES) {
            let caps: Vec<(String, i64)> = self
                .conn
                .prepare("SELECT key, version FROM capabilities")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            if !caps.is_empty() {
                let list = caps
                    .into_iter()
                    .map(|(k, v)| Owned::Struct(vec![(sym::KEY, Owned::String(k)), (sys::VERSION, Owned::Int(v))]))
                    .collect();
                self.push_inline(sym::CAPABILITIES, None, Owned::List(list), diag);
            }
        } else {
            diag.error("kdf.no_capabilities", || {
                "KDF database is missing the 'capabilities' table".into()
            });
        }

        for sql in tables {
            if !sql.starts_with("CREATE TABLE index_") {
                diag.error("kdf.schema", || format!("unexpected KDF database schema: {sql}"));
            }
        }

        self.push_inline(
            sym::CONTAINER,
            None,
            Owned::Struct(vec![
                (sym::APPLICATION_VERSION, Owned::String(String::new())),
                (sym::PACKAGE_VERSION, Owned::String(String::new())),
                (sym::FORMAT, Owned::String("KPF".into())),
            ]),
            diag,
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn process_row(
        &mut self,
        table: Table,
        rowid: i64,
        id: &str,
        payload_type: &str,
        len: u64,
        element_types: &HashMap<String, String>,
        have_symtab: &mut bool,
        diag: &mut Diagnostics,
    ) -> Result<()> {
        match payload_type {
            "blob" => {}
            "path" => {
                diag.error("kdf.path_payload", || {
                    format!("external resource payloads are not supported: {id}")
                });
                return Ok(());
            }
            _ => {
                diag.error("kdf.payload_type", || {
                    format!("unexpected payload_type={payload_type}, id={id}")
                });
                return Ok(());
            }
        }

        let head = self.read_blob(table, rowid, Some(HEAD_LEN))?;
        if head.starts_with(DRMION_SIGNATURE) {
            return Err(Error::Drm);
        }
        let row = |raw| Fragment {
            key: FragmentKey::new(0, None),
            db_id: id.into(),
            raw,
            len,
            payload: Payload::Row { table, rowid },
        };

        match id {
            "$ion_symbol_table" => {
                if !*have_symtab {
                    let data = self.read_blob(table, rowid, None)?;
                    let value = read_single(&data)?;
                    match value {
                        Value::Annotated(a) if a.first_annotation() == Ok(sys::ION_SYMBOL_TABLE) => {
                            self.symtab.apply_symbol_table(a.value()?, diag)?;
                        }
                        _ => {
                            return Err(Error::Invalid(
                                "$ion_symbol_table fragment is not a symbol table".into(),
                            ));
                        }
                    }
                    self.push(
                        Fragment {
                            key: FragmentKey::new(sys::ION_SYMBOL_TABLE, None),
                            ..row(false)
                        },
                        diag,
                    );
                    *have_symtab = true;
                }
                return Ok(());
            }
            "max_id" => {
                if !*have_symtab {
                    let data = self.read_blob(table, rowid, None)?;
                    let max_id = read_single(&data)?
                        .as_i64()
                        .ok_or(Error::Invalid("max_id is not an int".into()))?;
                    let yj_max = (max_id - ion::symtab::SYSTEM_SYMBOLS.len() as i64).clamp(0, i64::from(u32::MAX));
                    self.symtab = SymbolTable::with_yj_symbols(yj_max as u32, diag);
                    let import = Owned::Struct(vec![
                        (sys::MAX_ID, Owned::Int(max_id)),
                        (
                            sys::IMPORTS,
                            Owned::List(vec![Owned::Struct(vec![
                                (sys::NAME, Owned::String(ion::symtab::YJ_SYMBOLS_NAME.into())),
                                (sys::VERSION, Owned::Int(ion::symtab::YJ_SYMBOLS_VERSION)),
                                (sys::MAX_ID, Owned::Int(yj_max)),
                            ])]),
                        ),
                    ]);
                    self.push_inline(sys::ION_SYMBOL_TABLE, None, import, diag);
                    *have_symtab = true;
                }
                return Ok(());
            }
            "max_eid_in_sections" => {
                if !self.is_dictionary {
                    diag.warning("kdf.max_eid_in_sections", || {
                        "unexpected max_eid_in_sections for non-dictionary".into()
                    });
                }
                return Ok(());
            }
            _ => {}
        }

        if !head.starts_with(&SIGNATURE) {
            self.push(
                Fragment {
                    key: FragmentKey::new(sym::RAW_MEDIA, Some(id)),
                    ..row(true)
                },
                diag,
            );
            return Ok(());
        }

        if len == SIGNATURE.len() as u64 {
            if element_types.get(id).map(String::as_str) == Some("auxiliary_data") {
                let value = Owned::Struct(vec![
                    (
                        sym::KFX_ID,
                        Owned::Annotated(vec![sym::KFX_ID], Box::new(Owned::String(id.into()))),
                    ),
                    (sym::METADATA, Owned::List(vec![])),
                ]);
                self.push_inline(sym::AUXILIARY_DATA, Some(id), value, diag);
            } else if id != "book_navigation" {
                diag.warning("kdf.empty_fragment", || format!("ignoring empty {id} fragment"));
            }
            return Ok(());
        }

        let annotations = match peek_annotations(&head) {
            Err(ion::Error::Eof) if (head.len() as u64) < len => {
                peek_annotations(&self.read_blob(table, rowid, None)?)?
            }
            r => r?,
        };
        let Some(annotations) = annotations else {
            diag.error("kdf.no_annotation", || {
                format!("KDF fragment id={id} is missing its type annotation")
            });
            return Ok(());
        };
        if annotations.len() > 1 && !(annotations.len() == 2 && annotations[1] == sym::STRUCTURE) {
            diag.error("kdf.annotations", || {
                format!("KDF fragment id={id} should have one annotation")
            });
        }
        let ftype = annotations[0];
        let fid = if sym::ROOT_FRAGMENT_TYPES.contains(&ftype) {
            None
        } else {
            Some(id)
        };
        self.push(
            Fragment {
                key: FragmentKey::new(ftype, fid),
                ..row(false)
            },
            diag,
        );
        Ok(())
    }

    fn push_inline(&mut self, ftype: SymbolId, fid: Option<&str>, value: Owned, diag: &mut Diagnostics) {
        let bytes = to_document(&Owned::Annotated(vec![ftype], Box::new(value)));
        self.push(
            Fragment {
                key: FragmentKey::new(ftype, fid),
                db_id: "".into(),
                raw: false,
                len: bytes.len() as u64,
                payload: Payload::Inline(bytes),
            },
            diag,
        );
    }

    fn push(&mut self, fragment: Fragment, diag: &mut Diagnostics) {
        if self.index.contains_key(&fragment.key) {
            if ![sym::CONTAINER, sym::CAPABILITIES].contains(&fragment.ftype()) {
                diag.warning("kdf.duplicate", || format!("duplicate fragment {:?}", fragment.key));
            }
            return;
        }
        self.index.insert(fragment.key.clone(), self.fragments.len());
        self.fragments.push(fragment);
    }
}

/// The path of a plain SQLite database for `path`: the file itself, or a cached
/// copy without fingerprints. Also returns the number of fingerprints removed.
fn unwrapped_database(path: &Path, options: &OpenOptions, diag: &mut Diagnostics) -> Result<(PathBuf, usize)> {
    let mut file = fs::File::open(path)?;
    let mut head = [0u8; 2048];
    let n = read_up_to(&mut file, &mut head)?;
    let head = &head[..n];
    if !fingerprint::is_fingerprinted(head) {
        if !head.starts_with(SQLITE_SIGNATURE) {
            return Err(Error::NotKdf);
        }
        return Ok((path.to_path_buf(), 0));
    }

    // Streamed, so a large notebook is never held in memory: check every
    // fingerprint record, hash the file, and write the unwrapped copy once.
    let len = file.metadata()?.len() as usize;
    let offsets = fingerprint::offsets_for_len(len);
    let mut sig = [0u8; 4];
    for &pos in &offsets {
        file.seek(SeekFrom::Start(pos as u64))?;
        file.read_exact(&mut sig)?;
        if sig != fingerprint::SIGNATURE {
            diag.error("kdf.fingerprint", || {
                "unexpected fingerprint signature; using the file unchanged".into()
            });
            return Ok((path.to_path_buf(), 0));
        }
    }
    file.seek(SeekFrom::Start(0))?;
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(&mut file)?;

    let cache_dir = options
        .cache_dir
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("folio-kdf-cache"));
    let cached = cache_dir.join(format!("{}.sqlite", hasher.finalize().to_hex()));
    if !cached.exists() {
        fs::create_dir_all(&cache_dir)?;
        let part = cached.with_extension(format!("part{}", std::process::id()));
        {
            file.seek(SeekFrom::Start(0))?;
            let mut out = std::io::BufWriter::with_capacity(1 << 20, fs::File::create(&part)?);
            let mut pos = 0usize;
            for &skip in offsets.iter().chain(std::iter::once(&len)) {
                std::io::copy(&mut (&mut file).take((skip - pos) as u64), &mut out)?;
                if skip < len {
                    file.seek(SeekFrom::Current(fingerprint::RECORD_LEN as i64))?;
                }
                pos = skip + fingerprint::RECORD_LEN;
            }
            out.flush()?;
        }
        fs::rename(&part, &cached)?;
    }
    Ok((cached, offsets.len()))
}

fn read_up_to(file: &mut fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match file.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

fn connect(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        sqlite_uri(db_path),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.pragma_update(None, "mmap_size", 0)?;
    Ok(conn)
}

/// A read-only, immutable SQLite URI for `path` (no journal or lock files are created).
fn sqlite_uri(path: &Path) -> String {
    let mut uri = String::from("file:");
    for b in path.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri.push_str("?immutable=1");
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_escapes() {
        assert_eq!(
            sqlite_uri(Path::new("/a b/c?#.nbk")),
            "file:/a%20b/c%3F%23.nbk?immutable=1"
        );
    }
}
