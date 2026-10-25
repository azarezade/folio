//! Pushing files to the Kindle and deleting them.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use folio_device::{Handle, Kindle};

use crate::sync::{Kind, Manifest};
use crate::{Error, Layout, Result};

/// File types the Kindle reads that may be sent to it (as the Python app allowed).
pub const PUSH_EXTENSIONS: [&str; 6] = ["pdf", "epub", "mobi", "txt", "docx", "rtf"];

/// Resolves `dir` (e.g. `documents/Papers`), creating missing folders.
/// Only folders inside `documents/` are allowed.
fn ensure_dir(kindle: &Kindle, dir: &str) -> Result<Handle> {
    let parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    if parts.first().map(|p| p.to_ascii_lowercase()) != Some("documents".into()) {
        return Err(Error::Refused(format!("pushes go into documents/, not {dir}")));
    }
    if parts.iter().any(|p| *p == "." || *p == "..") {
        return Err(Error::Refused(format!("bad folder {dir}")));
    }
    let mut parent: Option<Handle> = None;
    for part in parts {
        let entries = kindle.list(parent)?;
        parent = Some(
            match entries
                .iter()
                .find(|e| e.is_folder && e.name.eq_ignore_ascii_case(part))
            {
                Some(e) => e.handle,
                None => kindle.mkdir(parent, part)?,
            },
        );
    }
    Ok(parent.expect("at least documents/"))
}

/// `name`, or `stem (2).ext`, `stem (3).ext`, … whichever is free.
fn free_name(taken: &[String], name: &str) -> String {
    let is_taken = |n: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(n));
    if !is_taken(name) {
        return name.to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    (2..)
        .map(|i| format!("{stem} ({i}){ext}"))
        .find(|n| !is_taken(n))
        .unwrap()
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct PushResult {
    pub device_path: String,
    pub bytes: u64,
}

/// Sends `src` to `target_dir` on the Kindle (default `documents`). An existing
/// file of the same name is kept and the new one gets a ` (2)` suffix.
pub fn push(
    kindle: &Kindle,
    src: &Path,
    target_dir: &str,
    progress: &mut (dyn FnMut(u64, u64) + Send),
    cancel: Option<&AtomicBool>,
) -> Result<PushResult> {
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !PUSH_EXTENSIONS.contains(&ext.as_str()) {
        return Err(Error::Refused(format!(
            "{name}: the Kindle takes {}",
            PUSH_EXTENSIONS.map(|e| format!(".{e}")).join(" ")
        )));
    }
    let size = std::fs::metadata(src)?.len();
    if size > kindle.info().free_bytes {
        return Err(Error::Refused(format!(
            "{name} needs {size} bytes; the Kindle has {} free",
            kindle.info().free_bytes
        )));
    }
    let dir = ensure_dir(kindle, target_dir)?;
    let taken: Vec<String> = kindle.list(Some(dir))?.into_iter().map(|e| e.name).collect();
    let target = free_name(&taken, &name);
    kindle.upload(src, Some(dir), &target, progress, cancel)?;

    // check it arrived whole
    let arrived = kindle.list(Some(dir))?.into_iter().find(|e| e.name == target);
    match arrived {
        Some(e) if e.size == size => {}
        Some(e) => {
            return Err(Error::Other(format!(
                "{target} arrived with {} of {size} bytes",
                e.size
            )));
        }
        None => return Err(Error::Other(format!("{target} is not listed after the upload"))),
    }
    let dir_path = target_dir.trim_matches('/');
    Ok(PushResult {
        device_path: format!("{dir_path}/{target}"),
        bytes: size,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteMode {
    /// Delete on the Kindle, then the local copy (only if the device delete worked).
    DeviceThenLocal,
    /// Remove only the local copy.
    LocalOnly,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct DeleteResult {
    pub device_path: String,
    pub deleted_on_device: bool,
    pub removed_locally: bool,
}

/// Deletes a synced book or screenshot. `target` is its library-relative path
/// or its device path. The device file must still be the synced one (same
/// path and size), so a different file that took its name is never deleted.
pub fn delete(kindle: Option<&Kindle>, layout: &Layout, target: &str, mode: DeleteMode) -> Result<DeleteResult> {
    let mut manifest = Manifest::load(layout);
    let key = manifest
        .files
        .iter()
        .find(|(local, e)| local.as_str() == target || e.device_path == target)
        .map(|(k, _)| k.clone())
        .ok_or_else(|| Error::Refused(format!("{target} is not a synced file")))?;
    let entry = manifest.files[&key].clone();
    if !matches!(entry.kind, Kind::Book | Kind::Screenshot) {
        return Err(Error::Refused(format!(
            "only books and screenshots can be deleted ({target} is a {:?})",
            entry.kind
        )));
    }

    let mut deleted_on_device = false;
    if mode == DeleteMode::DeviceThenLocal {
        let kindle = kindle.ok_or(Error::Device(folio_device::Error::NotConnected))?;
        let found = kindle.resolve(&entry.device_path)?;
        match found {
            Some(e) if !e.is_folder && e.size == entry.size => {
                kindle.delete(e.handle)?;
                if kindle.resolve(&entry.device_path)?.is_some() {
                    return Err(Error::Other(format!(
                        "{} is still on the Kindle after deleting it",
                        entry.device_path
                    )));
                }
                deleted_on_device = true;
            }
            Some(e) => {
                return Err(Error::Refused(format!(
                    "{} on the Kindle is {} bytes, not the {} that was synced; not deleting it",
                    entry.device_path, e.size, entry.size
                )));
            }
            None => {
                return Err(Error::Refused(format!(
                    "{} is no longer on the Kindle; use local-only removal",
                    entry.device_path
                )));
            }
        }
    }

    let local = layout.root.join(&key);
    let removed_locally = match std::fs::remove_file(&local) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.into()),
    };
    manifest.files.remove(&key);
    manifest.save(layout)?;
    Ok(DeleteResult {
        device_path: entry.device_path,
        deleted_on_device,
        removed_locally,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collision_names() {
        let taken = vec!["a.pdf".to_string(), "A (2).pdf".to_string()];
        assert_eq!(free_name(&taken, "b.pdf"), "b.pdf");
        assert_eq!(free_name(&taken, "a.pdf"), "a (3).pdf");
        assert_eq!(free_name(&["notes".to_string()], "notes"), "notes (2)");
    }
}
