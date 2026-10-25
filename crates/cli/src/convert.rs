//! `folio-cli convert`, `convert-all`, `render-page` and `bench`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use clap::Args;
use folio_core::export::{ExportMode, ExportOptions, export_pdf, render_page_image};
use folio_kdf::ion::diag::Level;
use folio_render::{GrayImage, PencilMode, RasterOptions};

use crate::Result;

#[derive(Args, Clone, Debug)]
pub struct RenderArgs {
    /// Vector PDF: strokes as paths, pencil as solid grey (default is raster).
    #[arg(long)]
    pub vector: bool,
    /// Raster: long edge of each page in pixels.
    #[arg(long, default_value_t = 2480, value_name = "PX")]
    pub long_edge: u32,
    /// Raster: resolution in dpi (sets --long-edge for the 8.27 in page height).
    #[arg(long, value_name = "N")]
    pub dpi: Option<u32>,
    /// Raster: canvas units per pencil-texture pixel (kfxlib used 8).
    #[arg(long, default_value_t = 4, value_parser = clap::value_parser!(u32).range(1..=16))]
    pub pencil_scale: u32,
    /// Raster: draw pressure-shaded strokes as solid grey instead of pencil texture.
    #[arg(long)]
    pub gray: bool,
    /// Leave out "convert to shape" drawings (kfxlib never drew them).
    #[arg(long)]
    pub no_shapes: bool,
}

impl RenderArgs {
    pub fn raster(&self) -> RasterOptions {
        RasterOptions {
            long_edge: self
                .dpi
                .map_or(self.long_edge, |d| (f64::from(d) * 2480.0 / 300.0).round() as u32),
            pencil: if self.gray {
                PencilMode::Gray
            } else {
                PencilMode::Texture {
                    scale: self.pencil_scale,
                }
            },
            shapes: !self.no_shapes,
        }
    }

    pub fn mode(&self) -> ExportMode {
        if self.vector {
            ExportMode::Vector {
                shapes: !self.no_shapes,
            }
        } else {
            ExportMode::Raster(self.raster())
        }
    }
}

/// Parses "1-10, 15" (1-based, inclusive) into 0-based page numbers.
pub fn parse_pages(spec: &str) -> Result<Vec<usize>> {
    let mut out = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim().parse::<usize>()?, b.trim().parse::<usize>()?),
            None => {
                let n = part.parse::<usize>()?;
                (n, n)
            }
        };
        if a == 0 || b < a {
            return Err(format!("bad page range '{part}'").into());
        }
        out.extend(a - 1..b);
    }
    Ok(out)
}

/// Peak resident memory of this process, in bytes.
pub fn peak_rss() -> u64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: getrusage only writes into the struct we pass.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
        return 0;
    }
    let maxrss = usage.ru_maxrss as u64;
    // macOS reports bytes, Linux kilobytes
    if cfg!(target_os = "macos") {
        maxrss
    } else {
        maxrss * 1024
    }
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

pub fn convert(
    input: &Path,
    output: &Path,
    render: &RenderArgs,
    jobs: usize,
    pages: Option<&str>,
    stats: bool,
) -> Result<()> {
    let opts = ExportOptions {
        mode: render.mode(),
        jobs,
        pages: pages.map(parse_pages).transpose()?,
        cache_dir: None,
    };
    let quiet = stats;
    let progress = |done: usize, total: usize| {
        if !quiet && (done.is_multiple_of(25) || done == total) {
            eprint!("\r  {done}/{total} pages");
            let _ = std::io::stderr().flush();
        }
    };
    let s = export_pdf(input, output, &opts, None, &progress)?;
    if !quiet {
        eprintln!();
    }
    if stats {
        // one machine-readable line, for `folio-cli bench`
        println!(
            "pages={} bytes={} seconds={:.3} peak_rss={}",
            s.pages,
            s.bytes,
            s.elapsed.as_secs_f64(),
            peak_rss()
        );
    } else {
        println!(
            "{} → {}: {} pages, {:.1} MB, {:.1} s, peak memory {:.0} MB",
            input.display(),
            output.display(),
            s.pages,
            mb(s.bytes),
            s.elapsed.as_secs_f64(),
            mb(peak_rss())
        );
        let shown: Vec<_> = s.diagnostics.iter().filter(|d| d.level > Level::Debug).collect();
        for d in &shown {
            println!("  {:?} ×{} {}: {}", d.level, d.count, d.kind, d.example);
        }
    }
    Ok(())
}

pub fn convert_all(dir: &Path, outdir: &Path, render: &RenderArgs, jobs: usize) -> Result<()> {
    std::fs::create_dir_all(outdir)?;
    let mut inputs: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "nbk"))
        .collect();
    inputs.sort();
    let (mut done, mut skipped, mut failed) = (0, 0, 0);
    for input in &inputs {
        let out = outdir.join(input.with_extension("pdf").file_name().unwrap());
        let up_to_date = match (
            std::fs::metadata(input).and_then(|m| m.modified()),
            std::fs::metadata(&out).and_then(|m| m.modified()),
        ) {
            (Ok(src), Ok(dst)) => dst >= src,
            _ => false,
        };
        if up_to_date {
            skipped += 1;
            println!("skip  {} (up to date)", out.display());
            continue;
        }
        match convert(input, &out, render, jobs, None, false) {
            Ok(()) => done += 1,
            Err(e) => {
                failed += 1;
                eprintln!("FAIL  {}: {e}", input.display());
            }
        }
    }
    println!("{done} converted, {skipped} up to date, {failed} failed");
    if failed > 0 {
        Err(format!("{failed} notebooks failed").into())
    } else {
        Ok(())
    }
}

pub fn write_png(img: &GrayImage, path: &Path) -> Result<()> {
    std::fs::write(path, img.to_png())?;
    Ok(())
}

pub fn render_page(input: &Path, page: usize, output: &Path, width: Option<u32>, render: &RenderArgs) -> Result<()> {
    if page == 0 {
        return Err("pages are numbered from 1".into());
    }
    let mut opts = render.raster();
    if let Some(w) = width {
        // portrait pages: width is the short edge (3:4)
        opts.long_edge = (f64::from(w) * 4.0 / 3.0).round() as u32;
    }
    let t = Instant::now();
    let img = render_page_image(input, page - 1, &opts)?;
    write_png(&img, output)?;
    println!(
        "{} ({}×{}) in {:.0} ms",
        output.display(),
        img.width,
        img.height,
        t.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

/// Runs `folio-cli convert --stats` in child processes so each stage's peak memory is its own.
pub fn bench(input: &Path, jobs: usize) -> Result<()> {
    let exe = std::env::current_exe()?;
    let tmp = std::env::temp_dir().join(format!("folio-bench-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;
    let stages: [(&str, &[&str]); 4] = [
        ("inspect + decode all pages", &[]),
        ("vector PDF", &["--vector"]),
        ("raster PDF, pencil scale 4", &[]),
        ("raster PDF, pencil scale 8", &["--pencil-scale", "8"]),
    ];
    println!("{}", input.display());
    for (i, (name, extra)) in stages.iter().enumerate() {
        let out = tmp.join(format!("stage{i}.pdf"));
        let mut cmd = Command::new(&exe);
        if i == 0 {
            cmd.args(["inspect", "--json", "--strokes"]).arg(input);
        } else {
            cmd.arg("convert")
                .arg(input)
                .arg(&out)
                .args(["--stats", "--jobs", &jobs.to_string()])
                .args(*extra);
        }
        let t = Instant::now();
        let output = cmd.output()?;
        let secs = t.elapsed().as_secs_f64();
        if !output.status.success() {
            println!(
                "  {name:<28} FAILED: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            continue;
        }
        let line = String::from_utf8_lossy(&output.stdout);
        let field = |k: &str| {
            line.split_whitespace()
                .find_map(|f| f.strip_prefix(&format!("{k}=")))
                .and_then(|v| v.parse::<f64>().ok())
        };
        match (field("peak_rss"), field("bytes")) {
            (Some(rss), Some(bytes)) => println!(
                "  {name:<28} {secs:>7.2} s   peak {:>6.0} MB   output {:>6.1} MB",
                rss / 1048576.0,
                bytes / 1048576.0
            ),
            _ => println!("  {name:<28} {secs:>7.2} s"),
        }
        let _ = std::fs::remove_file(&out);
    }
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_ranges() {
        assert_eq!(parse_pages("1-3, 5").unwrap(), vec![0, 1, 2, 4]);
        assert!(parse_pages("0").is_err());
        assert!(parse_pages("4-2").is_err());
    }
}
