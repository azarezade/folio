//! Library index, cache, thumbnails and the sync engine.
//!
//! The on-disk layout (kept from the Python app), book classification, the
//! index database ([`index`]), the render cache ([`cache`]), the sync engine
//! ([`sync`]) and pushing and deleting files on the Kindle ([`transfer`]).

pub mod cache;
pub mod index;
pub mod sync;
pub mod transfer;

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Device(#[from] folio_device::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Refused(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// The library folder, laid out as the Python app did:
/// `notebooks/<UUID>.nbk` + `.png`, `books/purchased|sideloaded/…`,
/// `screenshots/`; plus `annotations/` and `.sync/` for Phase 8 and the sync
/// manifest. Book subfolders on the device are kept (the Python app flattened them).
#[derive(Clone, Debug)]
pub struct Layout {
    pub root: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BookClass {
    Purchased,
    Sideloaded,
}

impl Layout {
    pub fn new(root: impl Into<PathBuf>) -> Layout {
        Layout { root: root.into() }
    }

    /// `~/Library/Application Support/Folio`. A library made before
    /// the app was renamed (`…/Scribe Library`) is moved here the first time.
    pub fn default_root() -> PathBuf {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let root = home.join("Library/Application Support/Folio");
        let old = home.join("Library/Application Support/Scribe Library");
        if !root.exists() && old.is_dir() {
            let _ = std::fs::rename(&old, &root);
        }
        root
    }

    pub fn notebooks_rel(&self) -> PathBuf {
        PathBuf::from("notebooks")
    }

    pub fn annotations_rel(&self) -> PathBuf {
        PathBuf::from("annotations")
    }

    pub fn books_rel(&self, class: BookClass) -> PathBuf {
        Path::new("books").join(match class {
            BookClass::Purchased => "purchased",
            BookClass::Sideloaded => "sideloaded",
        })
    }

    pub fn screenshots_rel(&self) -> PathBuf {
        PathBuf::from("screenshots")
    }

    pub fn sync_db_rel(&self) -> PathBuf {
        Path::new(".sync").join("KSDKNoteSyncDB.sqlite")
    }

    pub fn manifest(&self) -> PathBuf {
        self.root.join(".folio-sync.json")
    }

    /// The manifest's name before the app was called Folio; read if the new one is missing.
    pub fn legacy_manifest(&self) -> PathBuf {
        self.root.join(".scribe-sync.json")
    }
}

/// `B` + 9 alphanumerics, optionally `_EBOK`, `_EBSP`, `_SMPL`, `_PDOC` or `_HMRK`.
pub fn is_asin(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    let base = ["_EBOK", "_EBSP", "_SMPL", "_PDOC", "_HMRK"]
        .iter()
        .find_map(|s| up.strip_suffix(s))
        .unwrap_or(&up);
    base.len() == 10 && base.starts_with('B') && base.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
}

/// Purchased if the file's stem or its folder is ASIN-shaped, as
/// `library.py:classify_remote_file` decides; everything else is sideloaded.
pub fn classify_book(stem: &str, parent_folder: &str) -> BookClass {
    if is_asin(stem) || is_asin(parent_folder) {
        BookClass::Purchased
    } else {
        BookClass::Sideloaded
    }
}

/// A device file or folder name made safe as one local path component.
pub fn safe_component(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c == ':' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.').to_string();
    // keep dot-folders like .sdr contents readable, but never "", "." or ".."
    if trimmed.is_empty() { "_".into() } else { trimmed }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asins() {
        assert!(is_asin("B07XYZ1234"));
        assert!(is_asin("B07XYZ1234_EBOK"));
        assert!(is_asin("b07xyz1234_ebok"));
        assert!(!is_asin("B07XYZ123"));
        assert!(!is_asin("Oxford_Dictionary_of_English"));
        assert_eq!(classify_book("Notes", "B07XYZ1234_EBOK"), BookClass::Purchased);
        assert_eq!(
            classify_book("Oxford_Dictionary_of_English", "dictionaries"),
            BookClass::Sideloaded
        );
    }

    #[test]
    fn safe_names() {
        assert_eq!(safe_component("a/b:c"), "a_b_c");
        assert_eq!(safe_component(".."), "_");
        assert_eq!(safe_component("My Clippings.txt"), "My Clippings.txt");
    }
}
