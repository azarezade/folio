//! A minimal ZIP reader for EPUB metadata: the central directory, and stored
//! or deflated entries. No ZIP64, no encryption; EPUBs need neither.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use flate2::read::DeflateDecoder;

struct Entry {
    method: u16,
    compressed: u64,
    size: u64,
    local_offset: u64,
}

pub struct Zip {
    file: File,
    entries: HashMap<String, Entry>,
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn bad(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())
}

impl Zip {
    pub fn open(path: &Path) -> std::io::Result<Zip> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        // The end-of-central-directory record is in the last 22 + 65535 bytes.
        let tail_len = len.min(22 + 65535);
        file.seek(SeekFrom::Start(len - tail_len))?;
        let mut tail = vec![0; tail_len as usize];
        file.read_exact(&mut tail)?;
        let eocd = (0..tail.len().saturating_sub(21))
            .rev()
            .find(|&i| tail[i..i + 4] == *b"PK\x05\x06")
            .ok_or_else(|| bad("not a ZIP file"))?;
        let count = u16_at(&tail, eocd + 10) as usize;
        let cd_size = u32_at(&tail, eocd + 12) as u64;
        let cd_offset = u32_at(&tail, eocd + 16) as u64;
        if cd_offset + cd_size > len {
            return Err(bad("ZIP central directory out of range"));
        }
        file.seek(SeekFrom::Start(cd_offset))?;
        let mut cd = vec![0; cd_size as usize];
        file.read_exact(&mut cd)?;
        let mut entries = HashMap::new();
        let mut p = 0;
        for _ in 0..count {
            if p + 46 > cd.len() || cd[p..p + 4] != *b"PK\x01\x02" {
                break;
            }
            let name_len = u16_at(&cd, p + 28) as usize;
            let extra_len = u16_at(&cd, p + 30) as usize;
            let comment_len = u16_at(&cd, p + 32) as usize;
            let Some(name) = cd.get(p + 46..p + 46 + name_len) else {
                break;
            };
            entries.insert(
                String::from_utf8_lossy(name).into_owned(),
                Entry {
                    method: u16_at(&cd, p + 10),
                    compressed: u32_at(&cd, p + 20) as u64,
                    size: u32_at(&cd, p + 24) as u64,
                    local_offset: u32_at(&cd, p + 42) as u64,
                },
            );
            p += 46 + name_len + extra_len + comment_len;
        }
        Ok(Zip { file, entries })
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// The entry's bytes, if it exists and is at most `limit` bytes.
    pub fn read(&mut self, name: &str, limit: u64) -> std::io::Result<Option<Vec<u8>>> {
        let Some(e) = self.entries.get(name) else {
            return Ok(None);
        };
        if e.size > limit {
            return Err(bad("ZIP entry too large"));
        }
        let (method, compressed, size) = (e.method, e.compressed, e.size);
        self.file.seek(SeekFrom::Start(e.local_offset))?;
        let mut local = [0u8; 30];
        self.file.read_exact(&mut local)?;
        if local[..4] != *b"PK\x03\x04" {
            return Err(bad("bad ZIP local header"));
        }
        let skip = u16_at(&local, 26) as i64 + u16_at(&local, 28) as i64;
        self.file.seek(SeekFrom::Current(skip))?;
        let raw = (&mut self.file).take(compressed);
        let mut out = Vec::with_capacity(size as usize);
        match method {
            0 => raw.take(limit).read_to_end(&mut out)?,
            8 => DeflateDecoder::new(raw).take(limit).read_to_end(&mut out)?,
            m => return Err(bad(&format!("ZIP compression method {m}"))),
        };
        Ok(Some(out))
    }
}
