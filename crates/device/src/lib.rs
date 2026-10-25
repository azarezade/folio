//! Kindle over MTP: discovery, one session per use, transfers, and macOS
//! `ptpcamerad` handling.
//!
//! A blocking facade over the async `mtp-rs` crate; each [`Kindle`] owns a
//! private single-threaded runtime, so callers (CLI, sync engine, a UI worker
//! thread) need no async code. Device paths are `/`-separated from the storage
//! root, e.g. `documents/Papers/a.pdf` or `.notebooks/<uuid>/nbk`.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::ops::ControlFlow;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures::StreamExt;
use mtp_rs::mtp::{ByteRange, HotplugEvent, MtpDevice, MtpDeviceInfo, NewObjectInfo, ObjectHandle, Storage};

/// Amazon's USB vendor id (Lab126).
pub const KINDLE_VENDOR_ID: u16 = 0x1949;
/// Known Kindle Scribe product ids in MTP mode.
pub const SCRIBE_PRODUCT_IDS: [u16; 1] = [0x9981];

const CHUNK: usize = 1 << 20;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no Kindle is connected")]
    NotConnected,
    /// macOS (usually `ptpcamerad`) holds the device. See [`free_kindle`].
    #[error("the Kindle is in use by {}", holder.as_deref().unwrap_or("another process (probably ptpcamerad)"))]
    Busy { holder: Option<String> },
    #[error("not found on the Kindle: {0}")]
    NotFound(String),
    #[error("cancelled")]
    Cancelled,
    #[error("MTP: {0}")]
    Mtp(#[from] mtp_rs::mtp::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// A connected Kindle as USB reports it, before opening.
#[derive(Clone, Debug, serde::Serialize)]
pub struct UsbKindle {
    pub product_id: u16,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub location_id: u64,
}

impl UsbKindle {
    fn from(info: &MtpDeviceInfo) -> UsbKindle {
        UsbKindle {
            product_id: info.product_id,
            product: info.product.clone(),
            serial: info.serial_number.clone(),
            location_id: info.location_id,
        }
    }

    pub fn name(&self) -> &str {
        self.product.as_deref().unwrap_or("Kindle")
    }
}

/// Kindles currently on USB. Only Amazon's vendor id counts (the Python app
/// accepted any Amazon device); a Scribe sorts first when several are present.
pub fn list_kindles() -> Result<Vec<UsbKindle>> {
    let known: Vec<(u16, u16)> = SCRIBE_PRODUCT_IDS.iter().map(|&p| (KINDLE_VENDOR_ID, p)).collect();
    let mut out: Vec<UsbKindle> = MtpDevice::list_devices_with_known(&known)?
        .iter()
        .filter(|d| d.vendor_id == KINDLE_VENDOR_ID)
        .map(UsbKindle::from)
        .collect();
    out.sort_by_key(|k| !SCRIBE_PRODUCT_IDS.contains(&k.product_id));
    Ok(out)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
pub struct Handle(pub u64);

/// A file or folder on the Kindle. MTP reports no dates for Kindle objects.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Entry {
    pub handle: Handle,
    pub name: String,
    pub size: u64,
    pub is_folder: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct KindleInfo {
    pub name: String,
    pub product_id: u16,
    pub serial: Option<String>,
    pub storage: String,
    pub free_bytes: u64,
    pub total_bytes: u64,
}

/// An open MTP session with a Kindle, on the storage that has `documents/`.
pub struct Kindle {
    rt: tokio::runtime::Runtime,
    dev: Option<MtpDevice>,
    storage: Storage,
    info: KindleInfo,
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread().enable_all().build()?)
}

impl Kindle {
    /// Opens the first Kindle. A device held by another process gives [`Error::Busy`].
    pub fn open() -> Result<Kindle> {
        let target = list_kindles()?.into_iter().next().ok_or(Error::NotConnected)?;
        Kindle::open_usb(&target)
    }

    pub fn open_usb(target: &UsbKindle) -> Result<Kindle> {
        let rt = runtime()?;
        let known: Vec<(u16, u16)> = SCRIBE_PRODUCT_IDS.iter().map(|&p| (KINDLE_VENDOR_ID, p)).collect();
        let dev = rt
            .block_on(
                MtpDevice::builder()
                    .known_devices(&known)
                    .open_by_location(target.location_id),
            )
            .map_err(|e| {
                if e.is_exclusive_access() {
                    Error::Busy {
                        holder: exclusive_owner(target.name()),
                    }
                } else {
                    e.into()
                }
            })?;
        Kindle::from_device(rt, dev, target.name(), target.product_id, target.serial.clone())
    }

    /// A Kindle simulated by a folder (`mtp-rs`'s virtual device), for tests.
    #[cfg(feature = "virtual")]
    pub fn open_virtual(dir: &Path) -> Result<Kindle> {
        let rt = runtime()?;
        let config = mtp_rs::VirtualDeviceConfig {
            manufacturer: "Amazon".into(),
            model: "Kindle Scribe".into(),
            storages: vec![mtp_rs::VirtualStorageConfig {
                description: "Internal Storage".into(),
                capacity: 32_000_000_000,
                backing_dir: dir.to_path_buf(),
                read_only: false,
            }],
            ..Default::default()
        };
        let dev = rt.block_on(MtpDevice::builder().open_virtual(config))?;
        Kindle::from_device(rt, dev, "Kindle Scribe", SCRIBE_PRODUCT_IDS[0], None)
    }

    fn from_device(
        rt: tokio::runtime::Runtime,
        dev: MtpDevice,
        name: &str,
        product_id: u16,
        serial: Option<String>,
    ) -> Result<Kindle> {
        let storages = rt.block_on(dev.storages())?;
        // the storage with documents/ (the Scribe has one: "Internal Storage")
        let mut chosen = None;
        for s in storages {
            let root = rt.block_on(s.list_objects(None))?;
            if root
                .iter()
                .any(|o| o.is_folder() && o.filename.eq_ignore_ascii_case("documents"))
            {
                chosen = Some(s);
                break;
            }
        }
        let storage = chosen.ok_or_else(|| Error::NotFound("a storage with documents/".into()))?;
        let si = storage.info();
        let info = KindleInfo {
            name: name.to_string(),
            product_id,
            serial,
            storage: si.description.clone(),
            free_bytes: si.free_space,
            total_bytes: si.total_capacity,
        };
        Ok(Kindle {
            rt,
            dev: Some(dev),
            storage,
            info,
        })
    }

    pub fn info(&self) -> &KindleInfo {
        &self.info
    }

    /// Re-reads free space.
    pub fn refresh(&mut self) -> Result<&KindleInfo> {
        self.rt.block_on(self.storage.refresh())?;
        self.info.free_bytes = self.storage.info().free_space;
        self.info.total_bytes = self.storage.info().total_capacity;
        Ok(&self.info)
    }

    /// Lists a folder (`None` = storage root).
    pub fn list(&self, parent: Option<Handle>) -> Result<Vec<Entry>> {
        let objects = self
            .rt
            .block_on(self.storage.list_objects(parent.map(|h| ObjectHandle(h.0))))?;
        Ok(objects
            .into_iter()
            .map(|o| Entry {
                handle: Handle(o.handle.0),
                is_folder: o.is_folder(),
                name: o.filename,
                size: o.size,
            })
            .collect())
    }

    /// Finds an object by path from the storage root (case-sensitive, then case-insensitive).
    pub fn resolve(&self, path: &str) -> Result<Option<Entry>> {
        let mut parent: Option<Handle> = None;
        let mut found = None;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            let entries = self.list(parent)?;
            let hit = entries
                .iter()
                .find(|e| e.name == part)
                .or_else(|| entries.iter().find(|e| e.name.eq_ignore_ascii_case(part)))
                .cloned();
            let Some(e) = hit else { return Ok(None) };
            parent = Some(e.handle);
            found = Some(e);
        }
        Ok(found)
    }

    /// The first `len` bytes of a file (e.g. a notebook's SQLite header).
    pub fn read_head(&self, handle: Handle, len: u32) -> Result<Vec<u8>> {
        Ok(self
            .rt
            .block_on(self.storage.read_range(ObjectHandle(handle.0), 0, len))?)
    }

    /// Downloads to `dest` via `dest.part` and a rename. `progress(done, total)`.
    pub fn download(
        &self,
        handle: Handle,
        dest: &Path,
        progress: &mut dyn FnMut(u64, u64),
        cancel: Option<&AtomicBool>,
    ) -> Result<u64> {
        if let Some(dir) = dest.parent() {
            fs::create_dir_all(dir)?;
        }
        let part = dest.with_file_name(format!(
            "{}.part",
            dest.file_name().unwrap_or_default().to_string_lossy()
        ));
        let result = self.rt.block_on(async {
            let mut dl = self.storage.download(ObjectHandle(handle.0), ByteRange::Full).await?;
            let total = dl.size();
            let mut out = std::io::BufWriter::with_capacity(CHUNK, File::create(&part)?);
            let mut done = 0u64;
            while let Some(chunk) = dl.next_chunk().await {
                if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    let _ = dl.cancel(Duration::from_secs(2)).await;
                    return Err(Error::Cancelled);
                }
                let chunk = chunk?;
                out.write_all(&chunk)?;
                done += chunk.len() as u64;
                progress(done, total);
            }
            out.flush()?;
            out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
            Ok(done)
        });
        match result {
            Ok(n) => {
                fs::rename(&part, dest)?;
                Ok(n)
            }
            Err(e) => {
                let _ = fs::remove_file(&part);
                Err(e)
            }
        }
    }

    /// Uploads `src` into folder `parent` as `name`. On failure any partial
    /// object the device created is deleted.
    pub fn upload(
        &self,
        src: &Path,
        parent: Option<Handle>,
        name: &str,
        progress: &mut (dyn FnMut(u64, u64) + Send),
        cancel: Option<&AtomicBool>,
    ) -> Result<Handle> {
        let size = fs::metadata(src)?.len();
        let mut file = File::open(src)?;
        let chunks = std::iter::from_fn(move || {
            let mut buf = vec![0u8; CHUNK];
            match file.read(&mut buf) {
                Ok(0) => None,
                Ok(n) => {
                    buf.truncate(n);
                    Some(Ok(bytes::Bytes::from(buf)))
                }
                Err(e) => Some(Err(e)),
            }
        });
        let info = NewObjectInfo::file(name, size);
        let stream = futures::stream::iter(chunks);
        let result = self.rt.block_on(self.storage.upload_with_progress(
            parent.map(|h| ObjectHandle(h.0)),
            info,
            Box::pin(stream),
            |p| {
                progress(p.bytes_transferred, p.total_bytes.unwrap_or(size));
                if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
        ));
        match result {
            Ok(h) => Ok(Handle(h.0)),
            Err(e) => {
                if let Some(partial) = e.partial {
                    let _ = self.rt.block_on(self.storage.delete(partial));
                }
                if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    Err(Error::Cancelled)
                } else {
                    Err(e.source.into())
                }
            }
        }
    }

    pub fn delete(&self, handle: Handle) -> Result<()> {
        Ok(self.rt.block_on(self.storage.delete(ObjectHandle(handle.0)))?)
    }

    pub fn mkdir(&self, parent: Option<Handle>, name: &str) -> Result<Handle> {
        Ok(Handle(
            self.rt
                .block_on(self.storage.create_folder(parent.map(|h| ObjectHandle(h.0)), name))?
                .0,
        ))
    }

    /// Ends the session cleanly (also done on drop, best effort).
    pub fn close(mut self) -> Result<()> {
        if let Some(dev) = self.dev.take() {
            self.rt.block_on(dev.close())?;
        }
        Ok(())
    }
}

impl Drop for Kindle {
    fn drop(&mut self) {
        if let Some(dev) = self.dev.take() {
            let _ = self.rt.block_on(dev.close());
        }
    }
}

/// The process that holds the device exclusively, from the IORegistry
/// (`UsbExclusiveOwner`), e.g. `"pid 1234, ptpcamerad"`. macOS only.
pub fn exclusive_owner(product: &str) -> Option<String> {
    let out = Command::new("/usr/sbin/ioreg")
        .args(["-p", "IOUSB", "-l", "-w0", "-r", "-n", product])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .find(|l| l.contains("UsbExclusiveOwner"))
        .and_then(|l| l.split_once('=').map(|(_, v)| v.trim().trim_matches('"').to_string()))
}

/// "Free the Kindle": asks this account's `ptpcamerad` to quit
/// (SIGTERM; launchd starts it again later) and retries opening until
/// `timeout`. Returns the opened session.
pub fn free_kindle(timeout: Duration) -> Result<Kindle> {
    let start = Instant::now();
    loop {
        match Kindle::open() {
            Err(Error::Busy { holder }) => {
                if start.elapsed() > timeout {
                    return Err(Error::Busy { holder });
                }
                // only processes of this user can be signalled; that is the one that grabs USB
                let _ = Command::new("/usr/bin/pkill")
                    .args(["-TERM", "-x", "ptpcamerad"])
                    .status();
                std::thread::sleep(Duration::from_millis(300));
            }
            other => return other,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub enum DeviceEvent {
    Connected(UsbKindle),
    Disconnected(UsbKindle),
}

/// Watches for Kindles being plugged in and out (replaces the Python app's
/// 5-second USB polling). Calls `on_event` until it returns `false`.
pub fn watch(mut on_event: impl FnMut(DeviceEvent) -> bool) -> Result<()> {
    // The hot-plug stream runs on its own thread and runtime; events are handed
    // over a channel, so `on_event` runs outside any async context and may open
    // the Kindle (which starts a runtime of its own).
    let (tx, rx) = std::sync::mpsc::channel::<Result<DeviceEvent>>();
    std::thread::Builder::new().name("hotplug".into()).spawn(move || {
        let run = || -> Result<()> {
            let rt = runtime()?;
            let known: Vec<(u16, u16)> = SCRIBE_PRODUCT_IDS.iter().map(|&p| (KINDLE_VENDOR_ID, p)).collect();
            rt.block_on(async {
                let mut watch = mtp_rs::mtp::DeviceWatchBuilder::new().known_devices(&known).watch()?;
                while let Some(event) = watch.next().await {
                    let event = match event {
                        HotplugEvent::Arrived(d) if d.vendor_id == KINDLE_VENDOR_ID => {
                            DeviceEvent::Connected(UsbKindle::from(&d))
                        }
                        HotplugEvent::Left(d) if d.vendor_id == KINDLE_VENDOR_ID => {
                            DeviceEvent::Disconnected(UsbKindle::from(&d))
                        }
                        _ => continue,
                    };
                    if tx.send(Ok(event)).is_err() {
                        break; // the caller stopped watching
                    }
                }
                Ok(())
            })
        };
        if let Err(e) = run() {
            let _ = tx.send(Err(e));
        }
    })?;
    for event in rx {
        if !on_event(event?) {
            break;
        }
    }
    Ok(())
}
