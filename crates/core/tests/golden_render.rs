//! Phase 3 golden test: rendered pages must look like the kfxlib reference
//! renders (`fixtures/reference/*/png`), within an image-difference threshold.
//!
//! - `pNNNN_vector.png` (kfxlib's vector-gray mode, rasterised by cairo) is
//!   compared with our raster renderer in grey-path mode.
//! - `pNNNN_raster.png` (kfxlib's pencil texture at scale 8) is compared with
//!   our texture at scale 8. The textures are random, so pixels are compared
//!   after a 4×4 box filter, and total ink must agree.
//!
//! Shapes (`$272`) are left out: kfxlib never drew them. Without fixtures this
//! test passes with a note. `SCRIBE_GOLDEN_VERBOSE=1` prints every comparison.

use std::path::{Path, PathBuf};

use folio_core::export::render_page_image;
use folio_render::{GrayImage, PencilMode, RasterOptions};

/// Mean absolute difference allowed, on 0–255 grey, after the 4×4 box filter:
/// textures are random, grey paths should agree closely (measured: ≤ 2.6 and ≤ 0.2).
const MAX_MAE_TEXTURE: f64 = 4.0;
const MAX_MAE_PATHS: f64 = 1.0;
/// Pixels (after the filter) that may differ by more than 64.
const MAX_BAD_FRACTION: f64 = 0.01;
/// Relative difference allowed in total ink.
const MAX_INK_DIFF: f64 = 0.12;

fn native_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn load_png(path: &Path) -> GrayImage {
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!(info.color_type, png::ColorType::Grayscale, "{}", path.display());
    buf.truncate(info.buffer_size());
    GrayImage {
        width: info.width,
        height: info.height,
        data: buf,
    }
}

fn boxed(img: &GrayImage, k: u32) -> Vec<f64> {
    let (w, h) = (img.width / k, img.height / k);
    let mut out = vec![0.0; (w * h) as usize];
    for y in 0..h * k {
        for x in 0..w * k {
            out[((y / k) * w + x / k) as usize] += f64::from(img.data[(y * img.width + x) as usize]);
        }
    }
    out.iter_mut().for_each(|v| *v /= f64::from(k * k));
    out
}

struct Diff {
    mae: f64,
    bad: f64,
    ink: (f64, f64),
}

fn diff(ours: &GrayImage, theirs: &GrayImage) -> Diff {
    assert_eq!((ours.width, ours.height), (theirs.width, theirs.height));
    let (a, b) = (boxed(ours, 4), boxed(theirs, 4));
    let n = a.len() as f64;
    let mae = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).sum::<f64>() / n;
    let bad = a.iter().zip(&b).filter(|(x, y)| (*x - *y).abs() > 64.0).count() as f64 / n;
    let ink = |v: &[f64]| v.iter().map(|x| 255.0 - x).sum::<f64>() / n;
    Diff {
        mae,
        bad,
        ink: (ink(&a), ink(&b)),
    }
}

#[test]
fn renders_match_reference_pngs() {
    let root = native_root().join("fixtures/reference");
    let Ok(entries) = std::fs::read_dir(&root) else {
        eprintln!("no fixtures/reference dumps; skipping");
        return;
    };
    let verbose = std::env::var_os("SCRIBE_GOLDEN_VERBOSE").is_some();
    let mut jobs = Vec::new();
    for dir in entries.filter_map(|e| Some(e.ok()?.path())) {
        let Ok(summary) = std::fs::read_to_string(dir.join("summary.json")) else {
            continue;
        };
        let summary: serde_json::Value = serde_json::from_str(&summary).unwrap();
        let src = native_root().join("..").join(summary["source"].as_str().unwrap());
        for png in std::fs::read_dir(dir.join("png"))
            .unwrap()
            .filter_map(|e| Some(e.ok()?.path()))
        {
            let name = png.file_stem().unwrap().to_string_lossy().into_owned();
            let (page, kind) = name.trim_start_matches('p').split_once('_').unwrap();
            jobs.push((
                src.clone(),
                page.parse::<usize>().unwrap(),
                kind.to_string(),
                png.clone(),
            ));
        }
    }
    jobs.sort();

    let results: Vec<(String, Option<String>)> = std::thread::scope(|s| {
        let chunks: Vec<_> = jobs.chunks(jobs.len().div_ceil(8).max(1)).collect();
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| {
                s.spawn(move || {
                    chunk
                        .iter()
                        .map(|(src, page, kind, png)| {
                            let pencil = if kind == "raster" {
                                PencilMode::Texture { scale: 8 }
                            } else {
                                PencilMode::Gray
                            };
                            let opts = RasterOptions {
                                long_edge: 2480,
                                pencil,
                                shapes: false,
                            };
                            let ours = render_page_image(src, page - 1, &opts).unwrap();
                            let theirs = load_png(png);
                            let d = diff(&ours, &theirs);
                            let ink_diff = (d.ink.0 - d.ink.1).abs() / d.ink.1.max(1.0);
                            let label = format!(
                                "{} p{page} {kind}: mae {:.2}, bad {:.4}, ink ours {:.2} theirs {:.2}",
                                &src.file_stem().unwrap().to_string_lossy()[..8],
                                d.mae,
                                d.bad,
                                d.ink.0,
                                d.ink.1
                            );
                            let max_mae = if kind == "raster" {
                                MAX_MAE_TEXTURE
                            } else {
                                MAX_MAE_PATHS
                            };
                            let fail = (d.mae > max_mae
                                || d.bad > MAX_BAD_FRACTION
                                || (d.ink.1 > 0.5 && ink_diff > MAX_INK_DIFF))
                                .then(|| label.clone());
                            (label, fail)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });

    if verbose {
        for (label, _) in &results {
            eprintln!("{label}");
        }
    }
    let failures: Vec<&String> = results.iter().filter_map(|(_, f)| f.as_ref()).collect();
    assert!(
        failures.is_empty(),
        "{} renders differ:\n{}",
        failures.len(),
        failures.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n")
    );
    eprintln!("{} page renders match the reference", results.len());
}
