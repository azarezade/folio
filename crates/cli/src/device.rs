//! `folio-cli device`, `sync`, `push` and `delete`.

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use clap::Subcommand;
use folio_device::{DeviceEvent, Error as DeviceError, Kindle};
use folio_library::Layout;
use folio_library::sync::{self, Category, SyncOptions};
use folio_library::transfer::{self, DeleteMode};

use crate::Result;

#[derive(Subcommand)]
pub enum DeviceCommand {
    /// Ask macOS's ptpcamerad to let go of the Kindle, then connect.
    Free {
        #[arg(long, default_value_t = 10)]
        timeout: u64,
    },
    /// Print Kindles as they are plugged in and out (Ctrl-C to stop).
    Watch,
    /// List a folder on the Kindle (default: the storage root).
    Ls { path: Option<String> },
    /// Copy one file from the Kindle (into the current folder by default).
    Get { path: String, dest: Option<PathBuf> },
    /// Delete one file on the Kindle (not folders). Asks for --yes.
    Rm {
        path: String,
        #[arg(long)]
        yes: bool,
    },
}

fn mb(b: u64) -> f64 {
    b as f64 / 1_000_000.0
}

/// Opens the Kindle, explaining what to do when macOS holds it.
fn open() -> Result<Kindle> {
    match Kindle::open() {
        Err(DeviceError::Busy { holder }) => Err(format!(
            "the Kindle is in use by {}. Run `folio-cli device free` to ask macOS to let go of it.",
            holder.as_deref().unwrap_or("another process (probably ptpcamerad)")
        )
        .into()),
        Err(DeviceError::NotConnected) => Err("no Kindle is connected. Plug it in and unlock it.".into()),
        r => Ok(r?),
    }
}

fn print_status(k: &Kindle) {
    let i = k.info();
    println!(
        "{} (0x{:04x}) connected — {}: {:.1} GB free of {:.1} GB",
        i.name,
        i.product_id,
        i.storage,
        i.free_bytes as f64 / 1e9,
        i.total_bytes as f64 / 1e9
    );
}

pub fn device(command: Option<DeviceCommand>) -> Result<()> {
    match command {
        None => {
            let found = folio_device::list_kindles()?;
            if found.is_empty() {
                println!("no Kindle connected");
                return Ok(());
            }
            let k = open()?;
            print_status(&k);
            k.close()?;
        }
        Some(DeviceCommand::Free { timeout }) => {
            let k = folio_device::free_kindle(Duration::from_secs(timeout))?;
            print_status(&k);
            k.close()?;
        }
        Some(DeviceCommand::Watch) => {
            folio_device::watch(|e| {
                match e {
                    DeviceEvent::Connected(k) => println!("connected: {} (0x{:04x})", k.name(), k.product_id),
                    DeviceEvent::Disconnected(k) => println!("disconnected: {}", k.name()),
                }
                let _ = std::io::stdout().flush();
                true
            })?;
        }
        Some(DeviceCommand::Get { path, dest }) => {
            let k = open()?;
            let e = k.resolve(&path)?.ok_or_else(|| format!("not found: {path}"))?;
            if e.is_folder {
                return Err(format!("{path} is a folder").into());
            }
            let dest = match dest {
                Some(d) if d.is_dir() => d.join(&e.name),
                Some(d) => d,
                None => PathBuf::from(&e.name),
            };
            let start = std::time::Instant::now();
            let mut last = 0;
            let n = k.download(
                e.handle,
                &dest,
                &mut |done, total| {
                    if done - last >= 50_000_000 || done == total {
                        last = done;
                        eprint!("\r{:.0} / {:.0} MB", mb(done), mb(total));
                    }
                },
                None,
            )?;
            let secs = start.elapsed().as_secs_f64();
            eprintln!("\r{} → {} ({:.1} MB in {secs:.1} s)", path, dest.display(), mb(n));
            k.close()?;
        }
        Some(DeviceCommand::Rm { path, yes }) => {
            let k = open()?;
            let e = k.resolve(&path)?.ok_or_else(|| format!("not found: {path}"))?;
            if e.is_folder {
                return Err(format!("{path} is a folder; only files can be deleted").into());
            }
            if !yes {
                println!(
                    "this deletes {path} ({} bytes) from the Kindle. Run again with --yes to confirm.",
                    e.size
                );
                return Ok(());
            }
            k.delete(e.handle)?;
            println!("deleted {path}");
            k.close()?;
        }
        Some(DeviceCommand::Ls { path }) => {
            let k = open()?;
            let parent = match path.as_deref().filter(|p| !p.is_empty() && *p != "/") {
                Some(p) => Some(k.resolve(p)?.ok_or_else(|| format!("not found: {p}"))?.handle),
                None => None,
            };
            let mut entries = k.list(parent)?;
            entries.sort_by(|a, b| (!a.is_folder, &a.name).cmp(&(!b.is_folder, &b.name)));
            for e in entries {
                if e.is_folder {
                    println!("{:>12}  {}/", "", e.name);
                } else {
                    println!("{:>12}  {}", e.size, e.name);
                }
            }
            k.close()?;
        }
    }
    Ok(())
}

fn library(dir: Option<PathBuf>) -> Layout {
    Layout::new(dir.unwrap_or_else(Layout::default_root))
}

pub fn sync(
    notebooks: bool,
    books: bool,
    screenshots: bool,
    force: bool,
    sync_db: bool,
    dry_run: bool,
    dir: Option<PathBuf>,
) -> Result<()> {
    let layout = library(dir);
    std::fs::create_dir_all(&layout.root)?;
    let any = notebooks || books || screenshots;
    let opts = SyncOptions {
        notebooks: notebooks || !any,
        books: books || !any,
        screenshots: screenshots || !any,
        force,
        sync_db,
    };
    let k = open()?;
    print_status(&k);
    println!("library: {}", layout.root.display());
    let t = std::time::Instant::now();
    let plan = sync::plan(&k, &layout, &opts, &mut |m| eprintln!("  {m}"))?;
    println!("scanned the Kindle in {:.1} s", t.elapsed().as_secs_f64());
    for (cat, s) in plan.summary() {
        println!(
            "  {:<12} {:>4} found, {:>3} new, {:>3} changed, {:>4} up to date — {:.1} MB to copy",
            format!("{cat:?}"),
            s.found,
            s.new,
            s.changed,
            s.up_to_date,
            mb(s.bytes_to_copy)
        );
    }
    if dry_run {
        for i in plan.to_copy() {
            let why = i.reason.as_deref().map(|r| format!(" ({r})")).unwrap_or_default();
            println!("  {:?} {} → {}{why}", i.action, i.device_path, i.local.display());
        }
        return Ok(());
    }
    let mut last = String::new();
    let report = sync::run(
        &k,
        &layout,
        &plan,
        &mut |item, done, total| {
            let line = format!(
                "  ↓ {} {:.0}%",
                item.device_path,
                done as f64 * 100.0 / total.max(1) as f64
            );
            if line != last {
                eprint!("\r{line:<100}");
                last = line;
            }
        },
        None,
        &mut |m| eprintln!("\r{:<100}\r  {m}", ""),
    )?;
    k.close()?;
    for cat in [Category::Notebooks, Category::Books, Category::Screenshots] {
        if let Some(found) = report.found.get(&cat) {
            println!(
                "{:<12} {:>4} found, {:>3} copied, {:>4} up to date",
                format!("{cat:?}"),
                found,
                report.copied.get(&cat).unwrap_or(&0),
                report.skipped.get(&cat).unwrap_or(&0)
            );
        }
    }
    println!("{:.1} MB in {:.1} s", mb(report.bytes_copied), report.seconds);
    if !report.errors.is_empty() {
        return Err(format!("{} files failed", report.errors.len()).into());
    }
    Ok(())
}

pub fn push(files: &[PathBuf], to: &str) -> Result<()> {
    let k = open()?;
    for f in files {
        let res = transfer::push(
            &k,
            f,
            to,
            &mut |d, t| {
                eprint!("\r  ↑ {} {:.0}%", f.display(), d as f64 * 100.0 / t.max(1) as f64);
            },
            None,
        )?;
        eprintln!();
        println!("{} → {} ({:.1} MB)", f.display(), res.device_path, mb(res.bytes));
    }
    k.close()?;
    Ok(())
}

pub fn delete(target: &str, local_only: bool, yes: bool, dir: Option<PathBuf>) -> Result<()> {
    let layout = library(dir);
    if local_only {
        let r = transfer::delete(None, &layout, target, DeleteMode::LocalOnly)?;
        println!("removed the local copy of {}", r.device_path);
        return Ok(());
    }
    if !yes {
        return Err(format!("this deletes {target} from the Kindle. Run again with --yes to confirm.").into());
    }
    let k = open()?;
    let r = transfer::delete(Some(&k), &layout, target, DeleteMode::DeviceThenLocal)?;
    k.close()?;
    println!(
        "deleted {} on the Kindle{}",
        r.device_path,
        if r.removed_locally { " and the local copy" } else { "" }
    );
    Ok(())
}
