//! Books: format detection, DRM, metadata and covers.
//!
//! [`inspect`] looks at a book file's bytes (not its extension) and says what
//! it is, whether this app can open it, and its title, authors and cover.
//! MOBI/AZW/AZW3 are PalmDB files ([`mobi`]); EPUBs are ZIPs ([`epub`]); KFX
//! books are `CONT` containers, and encrypted ones are `DRMION` files. The
//! KFX → EPUB port is not done yet, so KFX books get a
//! clear reason instead.

pub mod epub;
pub mod mobi;
pub mod text;
mod xml;
mod zip;

use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Kind {
    Epub,
    /// MOBI 6 / AZW, possibly with a KF8 part.
    Mobi,
    /// KF8 only (AZW3).
    Kf8,
    Kfx,
    Pdf,
    Text,
    Docx,
    Unknown,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct BookInfo {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub asin: Option<String>,
    /// A Kindle dictionary (has an orthographic index).
    pub dictionary: bool,
    /// Why the book can't be opened here, if it can't.
    pub blocked: Option<String>,
    pub has_cover: bool,
}

pub struct Cover {
    pub mime: &'static str,
    pub data: Vec<u8>,
}

const DRM_KINDLE: &str = "This book is protected by Amazon DRM, so only a Kindle (or the Kindle app) can open it.";

/// The image type of `data` by its magic bytes.
pub fn image_mime(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if data.starts_with(b"\x89PNG") {
        Some("image/png")
    } else if data.starts_with(b"GIF8") {
        Some("image/gif")
    } else {
        None
    }
}

pub fn detect(path: &Path) -> std::io::Result<Kind> {
    let mut head = [0u8; 68];
    let n = File::open(path)?.read(&mut head)?;
    let head = &head[..n];
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    Ok(if head.starts_with(b"\xeaDRMION\xee") || head.starts_with(b"CONT") {
        Kind::Kfx
    } else if head.len() >= 68 && (&head[60..68] == b"BOOKMOBI" || &head[60..68] == b"TEXtREAd") {
        Kind::Mobi
    } else if head.starts_with(b"%PDF") {
        Kind::Pdf
    } else if head.starts_with(b"PK\x03\x04") {
        match ext.as_str() {
            "docx" => Kind::Docx,
            "kfx-zip" => Kind::Kfx,
            _ => Kind::Epub,
        }
    } else if ext == "txt" {
        Kind::Text
    } else {
        Kind::Unknown
    })
}

/// What the book is and whether it opens here. Errors reading the file
/// become a `blocked` reason, so every book gets an answer.
pub fn inspect(path: &Path) -> (Kind, BookInfo) {
    let kind = match detect(path) {
        Ok(k) => k,
        Err(e) => {
            return (
                Kind::Unknown,
                BookInfo {
                    blocked: Some(format!("The file can't be read: {e}.")),
                    ..Default::default()
                },
            );
        }
    };
    let result = match kind {
        Kind::Mobi => mobi::inspect(path).map(|(kf8_only, info)| (if kf8_only { Kind::Kf8 } else { Kind::Mobi }, info)),
        Kind::Epub => epub::inspect(path).map(|info| (kind, info)),
        Kind::Kfx => Ok((kind, kfx_info(path))),
        Kind::Docx => Ok((
            kind,
            BookInfo {
                blocked: Some(
                    "Word documents can't be opened here. The Kindle converts them when they are sent with Send to Kindle."
                        .into(),
                ),
                ..Default::default()
            },
        )),
        Kind::Unknown => Ok((
            kind,
            BookInfo {
                blocked: Some("This file isn't a book format the app recognises.".into()),
                ..Default::default()
            },
        )),
        Kind::Pdf | Kind::Text | Kind::Kf8 => Ok((kind, BookInfo::default())),
    };
    result.unwrap_or_else(|e| {
        (
            kind,
            BookInfo {
                blocked: Some(format!("The file looks damaged: {e}.")),
                ..Default::default()
            },
        )
    })
}

fn kfx_info(path: &Path) -> BookInfo {
    let drm = File::open(path)
        .and_then(|mut f| {
            let mut h = [0u8; 8];
            f.read_exact(&mut h).map(|_| h)
        })
        .is_ok_and(|h| h.starts_with(b"\xeaDRMION\xee"));
    BookInfo {
        blocked: Some(if drm {
            DRM_KINDLE.into()
        } else {
            "KFX books can't be opened here yet; the KFX → EPUB conversion is still to come.".into()
        }),
        ..Default::default()
    }
}

/// The book's cover image, if it has one.
pub fn cover(path: &Path) -> Option<Cover> {
    let data = match detect(path).ok()? {
        Kind::Mobi => mobi::cover(path).ok()??,
        Kind::Epub => epub::cover(path).ok()??,
        _ => return None,
    };
    Some(Cover {
        mime: image_mime(&data)?,
        data,
    })
}
