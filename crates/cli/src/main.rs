//! The `folio-cli` command-line tool.

mod convert;
mod device;
mod serve;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use folio_kdf::ion::diag::Level;
use folio_kdf::ion::text::{TextOptions, to_text};
use folio_kdf::{Kdf, OpenOptions};

#[derive(Parser)]
#[command(
    name = "folio-cli",
    version,
    about = "Folio: Kindle Scribe notebooks, books and sync from the command line"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Fragments, sections, page count, templates and a warnings summary.
    Inspect {
        file: PathBuf,
        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
        /// Decode every page and summarise strokes (and their warnings).
        #[arg(long, short)]
        strokes: bool,
        /// Also list harmless (debug-level) diagnostics.
        #[arg(long, short)]
        verbose: bool,
        /// Print the fragment(s) with this id (or type, e.g. `$538`) as Ion text.
        #[arg(long, value_name = "ID")]
        dump: Vec<String>,
    },
    /// Convert a notebook to PDF (raster by default; --vector for paths).
    Convert {
        input: PathBuf,
        output: PathBuf,
        #[command(flatten)]
        render: convert::RenderArgs,
        /// Worker threads (default: one per CPU).
        #[arg(long, short, default_value_t = 0)]
        jobs: usize,
        /// Pages to export, e.g. "1-10, 15".
        #[arg(long)]
        pages: Option<String>,
        /// Print one line of machine-readable stats (used by `bench`).
        #[arg(long, hide = true)]
        stats: bool,
    },
    /// Convert every .nbk in a folder, skipping up-to-date PDFs.
    ConvertAll {
        dir: PathBuf,
        outdir: PathBuf,
        #[command(flatten)]
        render: convert::RenderArgs,
        #[arg(long, short, default_value_t = 0)]
        jobs: usize,
    },
    /// Render one page (numbered from 1) to a PNG.
    RenderPage {
        input: PathBuf,
        page: usize,
        output: PathBuf,
        /// Page width in pixels (portrait); default long edge 2480.
        #[arg(long)]
        width: Option<u32>,
        #[command(flatten)]
        render: convert::RenderArgs,
    },
    /// Kindle status, or a device subcommand.
    Device {
        #[command(subcommand)]
        command: Option<device::DeviceCommand>,
    },
    /// Copy new and changed notebooks, books and screenshots from the Kindle
    /// (all three when none is chosen).
    Sync {
        #[arg(long)]
        notebooks: bool,
        #[arg(long)]
        books: bool,
        #[arg(long)]
        screenshots: bool,
        /// Copy everything, even what is up to date.
        #[arg(long)]
        force: bool,
        /// Also copy the Kindle's notebook sync database (.sync, ~900 MB).
        #[arg(long)]
        sync_db: bool,
        /// Only show what would be copied.
        #[arg(long, short = 'n')]
        dry_run: bool,
        /// Library folder (default ~/Library/Application Support/Folio).
        #[arg(long)]
        library: Option<PathBuf>,
    },
    /// Send files to the Kindle (.pdf .epub .mobi .txt .docx .rtf).
    Push {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Folder on the Kindle, inside documents/.
        #[arg(long, default_value = "documents")]
        to: String,
    },
    /// Delete a synced book or screenshot from the Kindle, then locally.
    Delete {
        /// Library-relative path or device path of the file.
        target: String,
        /// Remove only the local copy.
        #[arg(long)]
        local_only: bool,
        /// Confirm deleting on the Kindle.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        library: Option<PathBuf>,
    },
    /// Run the app core behind HTTP for developing the UI in a browser.
    Serve {
        #[arg(long, default_value_t = 8137)]
        port: u16,
        #[arg(long)]
        library: Option<PathBuf>,
        /// The built UI (default app/ui/dist).
        #[arg(long)]
        ui: Option<PathBuf>,
    },
    /// Time each stage and report peak memory.
    Bench {
        input: PathBuf,
        #[arg(long, short, default_value_t = 0)]
        jobs: usize,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Inspect {
            file,
            json,
            strokes,
            verbose,
            dump,
        } => {
            if dump.is_empty() {
                inspect(&file, json, strokes, verbose)
            } else {
                dump_fragments(&file, &dump)
            }
        }
        Command::Convert {
            input,
            output,
            render,
            jobs,
            pages,
            stats,
        } => convert::convert(&input, &output, &render, jobs, pages.as_deref(), stats),
        Command::ConvertAll {
            dir,
            outdir,
            render,
            jobs,
        } => convert::convert_all(&dir, &outdir, &render, jobs),
        Command::RenderPage {
            input,
            page,
            output,
            width,
            render,
        } => convert::render_page(&input, page, &output, width, &render),
        Command::Bench { input, jobs } => convert::bench(&input, jobs),
        Command::Device { command } => device::device(command),
        Command::Serve { port, library, ui } => serve::serve(port, library, ui),
        Command::Sync {
            notebooks,
            books,
            screenshots,
            force,
            sync_db,
            dry_run,
            library,
        } => device::sync(notebooks, books, screenshots, force, sync_db, dry_run, library),
        Command::Push { files, to } => device::push(&files, &to),
        Command::Delete {
            target,
            local_only,
            yes,
            library,
        } => device::delete(&target, local_only, yes, library),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("folio-cli: {e}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn inspect(file: &Path, json: bool, strokes: bool, verbose: bool) -> Result<()> {
    let r = folio_core::inspect(file, None, strokes)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&r)?);
        return Ok(());
    }
    println!("{}", r.path.display());
    println!(
        "  size          {} bytes, {} fingerprints removed",
        r.size, r.fingerprints_removed
    );
    println!("  notebook      {}", if r.is_notebook { "yes" } else { "no" });
    println!(
        "  fragments     {} stored, {} reachable",
        r.stored.total, r.reachable.total
    );
    for (t, n) in &r.reachable.by_type {
        let stored = r.stored.by_type.get(t).copied().unwrap_or(0);
        let note = if stored != *n {
            format!("  ({stored} stored)")
        } else {
            String::new()
        };
        println!("    {t:<18} {n:>6}{note}");
    }
    for ro in &r.reading_orders {
        println!(
            "  reading order {} — {} sections",
            ro.name.as_deref().unwrap_or("?"),
            ro.sections.len()
        );
    }
    println!("  sections      {} ({:?})", r.sections_processed, r.sections_by_kind);
    println!("  pages         {}", r.page_count);
    for (size, n) in &r.canvas_sizes {
        println!("    canvas {size:<12} {n:>6}");
    }
    for t in &r.templates {
        println!(
            "  template      {} = {}",
            t.section,
            t.template_type.as_deref().unwrap_or("?")
        );
    }
    if !r.unreferenced_pages.is_empty() {
        println!(
            "  deleted pages {} (still in the database, not shown)",
            r.unreferenced_pages.len()
        );
    }
    if let Some(s) = &r.strokes {
        println!(
            "  strokes       {} ({} points, {} groups, {} transformed)",
            s.strokes, s.points, s.stroke_groups, s.transformed
        );
        for (b, n) in &s.brushes {
            println!("    {b:<18} {n:>6}");
        }
        if s.graphics > 0 {
            println!(
                "  graphics      {} vector graphics (kfxlib skipped these): {:?}",
                s.graphics, s.shapes
            );
        }
        for (t, n) in &s.unhandled_content {
            println!("  skipped       {n} content nodes of type {t} (kfxlib skips them too)");
        }
        if let (true, Some(ex)) = (verbose, &s.unhandled_example) {
            println!("    e.g. {ex}");
        }
    }
    let shown: Vec<_> = r
        .diagnostics
        .iter()
        .filter(|d| verbose || d.level > Level::Debug)
        .collect();
    if !shown.is_empty() {
        println!("  diagnostics");
        for d in shown {
            println!("    {:?} ×{} {}: {}", d.level, d.count, d.kind, d.example);
        }
    }
    let hidden = r.diagnostics.iter().filter(|d| d.level == Level::Debug).count();
    if hidden > 0 && !verbose {
        println!("  ({hidden} kinds of harmless messages hidden; -v shows them)");
    }
    println!("  timing        {:?} ms", r.timing_ms);
    Ok(())
}

fn dump_fragments(file: &Path, ids: &[String]) -> Result<()> {
    let kdf = Kdf::open(file, &OpenOptions::default())?;
    for f in kdf.fragments() {
        let type_name = kdf.symtab().name(f.ftype());
        let matches = ids
            .iter()
            .any(|id| f.fid() == Some(id) || *id == type_name || *f.db_id == **id);
        if !matches {
            continue;
        }
        let data = kdf.read(f)?;
        let text = to_text(data.value()?, kdf.symtab(), TextOptions::default());
        println!("{}::{} = {text}", type_name, f.fid().unwrap_or("-"));
    }
    Ok(())
}
