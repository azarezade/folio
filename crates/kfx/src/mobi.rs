//! MOBI / AZW / AZW3 (PalmDB) headers: DRM, EXTH metadata and the cover.
//!
//! Layout (offsets in record 0): 0 compression, 12 encryption, 16 `MOBI`,
//! 20 header length, 28 text encoding, 36 version, 40 orthographic index
//! (dictionaries), 84/88 full name offset/length, 108 first image record,
//! 128 EXTH flags; the EXTH block follows the MOBI header. Images are never
//! encrypted, so DRM books still have covers.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::{BookInfo, DRM_KINDLE};

fn be16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn be32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn bad(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())
}

pub(crate) struct Palm {
    file: File,
    len: u64,
    offsets: Vec<u64>,
}

impl Palm {
    pub(crate) fn open(path: &Path) -> std::io::Result<Palm> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let mut head = [0u8; 78];
        file.read_exact(&mut head)?;
        let n = be16(&head, 76).unwrap_or(0) as usize;
        let mut table = vec![0u8; n * 8];
        file.read_exact(&mut table)?;
        let offsets = (0..n).map(|i| be32(&table, i * 8).unwrap_or(0) as u64).collect();
        Ok(Palm { file, len, offsets })
    }

    pub(crate) fn record(&mut self, i: usize, limit: u64) -> std::io::Result<Vec<u8>> {
        let start = *self.offsets.get(i).ok_or_else(|| bad("record out of range"))?;
        let end = self.offsets.get(i + 1).copied().unwrap_or(self.len);
        if end < start || end > self.len {
            return Err(bad("bad record offsets"));
        }
        self.file.seek(SeekFrom::Start(start))?;
        let mut buf = vec![0; (end - start).min(limit) as usize];
        self.file.read_exact(&mut buf)?;
        Ok(buf)
    }
}

struct Header {
    encryption: u16,
    version: u32,
    dictionary: bool,
    utf8: bool,
    full_name: Option<String>,
    first_image: Option<u32>,
    exth: Vec<(u32, Vec<u8>)>,
}

fn text(b: &[u8], utf8: bool) -> String {
    let s = if utf8 {
        String::from_utf8_lossy(b).into_owned()
    } else {
        // CP1252, close enough: Latin-1 for the rest.
        b.iter().map(|&c| c as char).collect()
    };
    s.trim_matches(char::from(0)).trim().to_string()
}

fn header(r0: &[u8]) -> std::io::Result<Header> {
    let encryption = be16(r0, 12).ok_or_else(|| bad("short record 0"))?;
    if r0.get(16..20) != Some(b"MOBI") {
        // PalmDoc (TEXtREAd) has no MOBI header.
        return Ok(Header {
            encryption,
            version: 0,
            dictionary: false,
            utf8: false,
            full_name: None,
            first_image: None,
            exth: Vec::new(),
        });
    }
    let hlen = be32(r0, 20).unwrap_or(0) as usize;
    let utf8 = be32(r0, 28) == Some(65001);
    let orth = be32(r0, 40).unwrap_or(u32::MAX);
    let full_name = match (be32(r0, 84), be32(r0, 88)) {
        (Some(o), Some(l)) => r0
            .get(o as usize..(o as usize).saturating_add(l as usize))
            .map(|b| text(b, utf8)),
        _ => None,
    };
    let first_image = be32(r0, 108).filter(|&i| i != u32::MAX && i != 0);
    let mut exth = Vec::new();
    if be32(r0, 128).is_some_and(|f| f & 0x40 != 0) {
        let e = 16 + hlen;
        if r0.get(e..e + 4) == Some(b"EXTH") {
            let count = be32(r0, e + 8).unwrap_or(0);
            let mut p = e + 12;
            for _ in 0..count {
                let (Some(t), Some(l)) = (be32(r0, p), be32(r0, p + 4)) else {
                    break;
                };
                let l = l as usize;
                let Some(v) = r0.get(p + 8..p + l.max(8)) else { break };
                exth.push((t, v.to_vec()));
                p += l.max(8);
            }
        }
    }
    Ok(Header {
        encryption,
        version: be32(r0, 36).unwrap_or(0),
        dictionary: orth != u32::MAX,
        utf8,
        full_name,
        first_image,
        exth,
    })
}

/// Returns (KF8 only, info).
pub fn inspect(path: &Path) -> std::io::Result<(bool, BookInfo)> {
    let mut palm = Palm::open(path)?;
    let r0 = palm.record(0, 1 << 20)?;
    let h = header(&r0)?;
    let get = |t: u32| {
        h.exth
            .iter()
            .filter(move |(k, _)| *k == t)
            .map(|(_, v)| text(v, h.utf8))
    };
    let title = get(503).next().or(h.full_name.clone()).filter(|t| !t.is_empty());
    let mut authors: Vec<String> = get(100).filter(|a| !a.is_empty()).collect();
    authors.dedup();
    let has_cover = h.first_image.is_some() && h.exth.iter().any(|(t, v)| *t == 201 && be32(v, 0) != Some(u32::MAX));
    let blocked = (h.encryption != 0).then(|| {
        if h.dictionary {
            "This is a Kindle dictionary protected by Amazon DRM. The Kindle uses it for look-ups; it can't be opened here."
                .to_string()
        } else {
            DRM_KINDLE.to_string()
        }
    });
    let info = BookInfo {
        title,
        authors,
        language: get(524).next(),
        asin: get(113).next().or_else(|| get(504).next()),
        dictionary: h.dictionary,
        blocked,
        has_cover,
    };
    Ok((h.version == 8, info))
}

pub fn cover(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    let mut palm = Palm::open(path)?;
    let r0 = palm.record(0, 1 << 20)?;
    let h = header(&r0)?;
    let Some(first) = h.first_image else { return Ok(None) };
    let offset = h
        .exth
        .iter()
        .find(|(t, _)| *t == 201)
        .and_then(|(_, v)| be32(v, 0))
        .filter(|&o| o != u32::MAX);
    let Some(offset) = offset else { return Ok(None) };
    let data = palm.record(first as usize + offset as usize, 16 << 20)?;
    Ok(crate::image_mime(&data).map(|_| data))
}
