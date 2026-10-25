//! The render cache (`~/Library/Caches/Folio`): page images and
//! thumbnails, keyed by the item's content hash, the renderer version and the
//! render options, so an edited notebook or a renderer upgrade never shows a
//! stale page (the Python app never invalidated). Files are written to a
//! `.part` and renamed, so a half-written entry is never read. The cache has a
//! size cap; the least recently used files go first.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::Result;

/// Reader page widths are rounded up to a multiple of this, so zooming reuses renders.
pub const WIDTH_BUCKET: u32 = 256;
pub const MAX_WIDTH: u32 = 4096;

pub fn width_bucket(width: u32) -> u32 {
    width.clamp(1, MAX_WIDTH).div_ceil(WIDTH_BUCKET) * WIDTH_BUCKET
}

#[derive(Clone, Debug)]
pub struct Cache {
    pub root: PathBuf,
}

impl Cache {
    pub fn new(root: impl Into<PathBuf>) -> Cache {
        Cache { root: root.into() }
    }

    pub fn default_root() -> PathBuf {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        home.join("Library/Caches/Folio")
    }

    /// Where a rendered page lives. `variant` encodes the render options.
    pub fn page_key(&self, content_hash: &str, renderer: u32, variant: &str, page: u32, width: u32) -> PathBuf {
        self.root
            .join("pages")
            .join(&content_hash[..2.min(content_hash.len())])
            .join(content_hash)
            .join(format!("r{renderer}-{variant}"))
            .join(format!("{page:05}-{width}.png"))
    }

    pub fn thumb_key(&self, content_hash: &str, renderer: u32) -> PathBuf {
        self.root.join("thumbs").join(format!("{content_hash}-r{renderer}.png"))
    }

    /// Reads an entry and marks it recently used.
    pub fn get(&self, key: &Path) -> Option<Vec<u8>> {
        let data = std::fs::read(key).ok()?;
        if let Ok(f) = std::fs::File::options().write(true).open(key) {
            let _ = f.set_modified(SystemTime::now());
        }
        Some(data)
    }

    pub fn put(&self, key: &Path, data: &[u8]) -> Result<()> {
        if let Some(dir) = key.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let part = key.with_extension(format!("part{}", std::process::id()));
        std::fs::write(&part, data)?;
        std::fs::rename(&part, key)?;
        Ok(())
    }

    fn files(&self) -> Vec<(PathBuf, u64, SystemTime)> {
        let mut out = Vec::new();
        let mut stack = vec![self.root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let Ok(meta) = e.metadata() else { continue };
                if meta.is_dir() {
                    stack.push(e.path());
                } else {
                    out.push((e.path(), meta.len(), meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
                }
            }
        }
        out
    }

    /// Total size in bytes.
    pub fn size(&self) -> u64 {
        self.files().iter().map(|f| f.1).sum()
    }

    /// Deletes the least recently used files until the cache is under 90 % of
    /// `cap_bytes`. Returns the bytes freed.
    pub fn prune(&self, cap_bytes: u64) -> u64 {
        let mut files = self.files();
        let total: u64 = files.iter().map(|f| f.1).sum();
        if total <= cap_bytes {
            return 0;
        }
        files.sort_by_key(|f| f.2);
        let target = cap_bytes / 10 * 9;
        let mut size = total;
        let mut freed = 0;
        for (path, len, _) in files {
            if size <= target {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                size -= len;
                freed += len;
            }
        }
        freed
    }

    pub fn clear(&self) -> Result<u64> {
        let size = self.size();
        if self.root.exists() {
            std::fs::remove_dir_all(&self.root)?;
        }
        Ok(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn buckets() {
        assert_eq!(width_bucket(1), 256);
        assert_eq!(width_bucket(256), 256);
        assert_eq!(width_bucket(257), 512);
        assert_eq!(width_bucket(99_999), 4096);
    }

    #[test]
    fn lru_prune() {
        let root = std::env::temp_dir().join(format!("folio-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let c = Cache::new(&root);
        let keys: Vec<PathBuf> = (0..4).map(|i| c.page_key("abcdef", 1, "tex4", i, 512)).collect();
        for (i, k) in keys.iter().enumerate() {
            c.put(k, &[0u8; 1000]).unwrap();
            let f = std::fs::File::options().write(true).open(k).unwrap();
            f.set_modified(SystemTime::now() - Duration::from_secs(100 - i as u64 * 10))
                .unwrap();
        }
        assert_eq!(c.size(), 4000);
        assert!(c.get(&keys[0]).is_some(), "reading marks the oldest as recently used");
        let freed = c.prune(3000);
        assert_eq!(freed, 2000);
        assert!(keys[0].exists(), "recently read entry kept");
        assert!(!keys[1].exists() && !keys[2].exists());
        assert!(keys[3].exists());
        c.clear().unwrap();
        assert_eq!(c.size(), 0);
    }
}
