//! KDF notebook container (SQLite) to a lazy fragment store.
//!
//! A port of `kfxlib/kpf_container.py` plus the fragment-reachability check
//! from `yj_structure.py`. See docs/format.md §1.

mod container;
pub mod fingerprint;
pub mod sym;
pub mod usage;

pub use container::{DRMION_SIGNATURE, Fragment, FragmentData, FragmentKey, Kdf, OpenOptions, Table};
pub use folio_ion as ion;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a KDF (SQLite) file")]
    NotKdf,
    #[error("DRM-protected")]
    Drm,
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Ion(#[from] folio_ion::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
