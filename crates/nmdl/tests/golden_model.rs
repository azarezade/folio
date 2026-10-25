//! Phase 2 golden test: every decoded page and stroke must equal the kfxlib
//! reference harness's `model.json.gz` exactly (points, widths, densities, raw
//! arrays and stroke metadata), for every notebook in `fixtures/reference/`.
//!
//! The models are large (up to 150 MB gzipped), so they are streamed one page
//! at a time. Without fixtures this test passes with a note.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use folio_kdf::OpenOptions;
use folio_kdf::ion::Diagnostics;
use folio_nmdl::{ARRAY_NAMES, Notebook, Page};
use serde::Deserialize;
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};

#[derive(Deserialize, Default)]
#[serde(default)]
struct HPage {
    index: usize,
    section: String,
    canvas: (i64, i64),
    normalized_ppi: Option<f64>,
    template_id: Option<String>,
    template_id_explicit: bool,
    inline_placement_type: Option<String>,
    stroke_groups: usize,
    strokes: Vec<HStroke>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct HStroke {
    id: Option<String>,
    nmdl_type: Option<String>,
    brush_type: Option<i64>,
    brush_name: String,
    color: Option<i64>,
    thickness: f64,
    seed: Option<i64>,
    bounds: Vec<i64>,
    num_points: usize,
    variable_density: bool,
    variable_thickness: bool,
    transform: Option<Vec<f64>>,
    extra_fields: BTreeMap<String, serde_json::Value>,
    extra_point_fields: Vec<String>,
    arrays: BTreeMap<String, Option<Vec<i64>>>,
    points: Vec<(i64, i64, i64, f64)>,
}

fn native_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

const MAX_REPORTED: usize = 20;

struct Checker<'a> {
    name: String,
    notebook: &'a Notebook,
    pages_seen: usize,
    strokes: usize,
    points: usize,
    graphics: usize,
    problems: Vec<String>,
}

impl Checker<'_> {
    fn problem(&mut self, msg: String) {
        if self.problems.len() < MAX_REPORTED {
            self.problems.push(format!("{}: {msg}", self.name));
        } else if self.problems.len() == MAX_REPORTED {
            self.problems.push(format!("{}: … more", self.name));
        }
    }

    fn check_page(&mut self, h: HPage) {
        let i = self.pages_seen;
        self.pages_seen += 1;
        if i >= self.notebook.page_count() {
            self.problem(format!(
                "harness has page {} but we have {}",
                h.index,
                self.notebook.page_count()
            ));
            return;
        }
        let page = match self.notebook.page(i, &mut Diagnostics::default()) {
            Ok(p) => p,
            Err(e) => return self.problem(format!("page {}: {e}", i + 1)),
        };
        self.graphics += page.graphics.len();
        if !page.unhandled.is_empty() {
            self.problem(format!(
                "page {}: {} unhandled content nodes",
                page.index,
                page.unhandled.len()
            ));
        }
        self.compare_page(&h, &page);
    }

    fn compare_page(&mut self, h: &HPage, p: &Page) {
        let at = format!("page {}", p.index);
        macro_rules! eq {
            ($what:expr, $ours:expr, $theirs:expr) => {
                if $ours != $theirs {
                    self.problem(format!(
                        "{at}: {}: ours {:?}, harness {:?}",
                        $what, $ours, $theirs
                    ));
                }
            };
        }
        eq!("index", p.index, h.index);
        eq!("section", p.section, h.section);
        eq!("canvas", p.canvas, h.canvas);
        eq!("normalized_ppi", p.normalized_ppi, h.normalized_ppi);
        eq!("template_id", p.template_id, h.template_id);
        eq!("template_id_explicit", p.template_id_explicit, h.template_id_explicit);
        eq!(
            "inline_placement_type",
            p.inline_placement_type,
            h.inline_placement_type
        );
        eq!("stroke_groups", p.stroke_groups, h.stroke_groups);
        let theirs: Vec<&HStroke> = h.strokes.iter().filter(|s| s.nmdl_type.is_none()).collect();
        eq!("stroke count", p.strokes.len(), theirs.len());

        for (k, (s, hs)) in p.strokes.iter().zip(theirs).enumerate() {
            self.strokes += 1;
            self.points += s.points.len();
            let at = format!("{at} stroke {k}");
            macro_rules! seq {
                ($what:expr, $ours:expr, $theirs:expr) => {
                    if $ours != $theirs {
                        self.problem(format!(
                            "{at}: {}: ours {:?}, harness {:?}",
                            $what, $ours, $theirs
                        ));
                    }
                };
            }
            seq!("id", s.id, hs.id);
            seq!("brush_type", s.brush_type, hs.brush_type);
            seq!("brush", s.brush.name(), hs.brush_name);
            seq!("color", s.color, hs.color);
            seq!("thickness", s.thickness, hs.thickness);
            seq!("seed", s.seed, hs.seed);
            seq!("bounds", s.bounds.to_vec(), hs.bounds);
            seq!("num_points", s.num_points, hs.num_points);
            seq!("variable_density", s.variable_density, hs.variable_density);
            seq!("variable_thickness", s.variable_thickness, hs.variable_thickness);
            seq!("transform", s.transform, hs.transform);
            seq!("extra_point_fields", s.extra_point_fields, hs.extra_point_fields);
            let ours_extra: BTreeMap<&str, &str> =
                s.extra_fields.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            let keys: Vec<&str> = hs.extra_fields.keys().map(String::as_str).collect();
            seq!(
                "extra field names",
                ours_extra.keys().copied().collect::<Vec<_>>(),
                keys
            );
            for (k, v) in &hs.extra_fields {
                if let (Some(theirs), Some(ours)) = (v.as_str(), ours_extra.get(k.as_str())) {
                    seq!(format!("extra field {k}"), *ours, theirs);
                }
            }
            for name in ARRAY_NAMES {
                seq!(
                    format!("array {name}"),
                    s.arrays.get(name),
                    hs.arrays.get(name).and_then(|v| v.as_ref())
                );
            }
            let points: Vec<(i64, i64, i64, f64)> = s.points.iter().map(|p| (p.x, p.y, p.t, p.d)).collect();
            if points != hs.points {
                let first = points.iter().zip(&hs.points).position(|(a, b)| a != b);
                self.problem(format!(
                    "{at}: points differ ({} vs {}), first at {first:?}: ours {:?}, harness {:?}",
                    points.len(),
                    hs.points.len(),
                    first.map(|i| points[i]),
                    first.map(|i| hs.points[i])
                ));
            }
        }
    }
}

/// Visits `{"harness_version", "notebook", "pages": [...]}`, checking pages as they stream past.
struct ModelSeed<'c, 'a>(&'c mut Checker<'a>);

impl<'de> DeserializeSeed<'de> for ModelSeed<'_, '_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for ModelSeed<'_, '_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a harness model")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            if key == "pages" {
                map.next_value_seed(PagesSeed(&mut *self.0))?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(())
    }
}

struct PagesSeed<'c, 'a>(&'c mut Checker<'a>);

impl<'de> DeserializeSeed<'de> for PagesSeed<'_, '_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for PagesSeed<'_, '_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a list of pages")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while let Some(page) = seq.next_element::<HPage>()? {
            self.0.check_page(page);
        }
        Ok(())
    }
}

fn check_notebook(dir: &Path) -> (Vec<String>, usize, usize, usize) {
    let summary: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("summary.json")).unwrap()).unwrap();
    let src = Path::new(summary["source"].as_str().unwrap());
    let src = if src.is_absolute() {
        src.to_path_buf()
    } else {
        native_root().join("..").join(src)
    };
    let name = dir.file_name().unwrap().to_string_lossy()[..8.min(dir.file_name().unwrap().len())].to_string();

    let notebook = match Notebook::open(&src, &OpenOptions::default(), &mut Diagnostics::default()) {
        Ok(n) => n,
        Err(e) => return (vec![format!("{name}: open failed: {e}")], 0, 0, 0),
    };
    let mut checker = Checker {
        name,
        notebook: &notebook,
        pages_seen: 0,
        strokes: 0,
        points: 0,
        graphics: 0,
        problems: Vec::new(),
    };
    let reader = BufReader::with_capacity(1 << 20, GzDecoder::new(File::open(dir.join("model.json.gz")).unwrap()));
    let mut de = serde_json::Deserializer::from_reader(reader);
    if let Err(e) = ModelSeed(&mut checker).deserialize(&mut de) {
        checker.problem(format!("reading model: {e}"));
    }
    if checker.pages_seen != notebook.page_count() {
        let (a, b) = (notebook.page_count(), checker.pages_seen);
        checker.problem(format!("page count: ours {a}, harness {b}"));
    }
    // kfxlib skips each $272 vector graphic with "has unknown content type"; we decode them.
    let skipped: u64 = summary["kfxlib_log"]["errors"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            e["message"]
                .as_str()
                .unwrap_or("")
                .ends_with("has unknown content type: #")
        })
        .map(|e| e["count"].as_u64().unwrap())
        .sum();
    if checker.graphics as u64 != skipped {
        let ours = checker.graphics;
        checker.problem(format!(
            "graphics: ours {ours}, kfxlib skipped {skipped} unknown content nodes"
        ));
    }
    (checker.problems, checker.pages_seen, checker.strokes, checker.points)
}

#[test]
fn model_matches_reference_harness() {
    let root = native_root().join("fixtures/reference");
    let Ok(entries) = std::fs::read_dir(&root) else {
        eprintln!("no fixtures/reference dumps; skipping (see tools/reference/README.md)");
        return;
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| p.join("model.json.gz").exists())
        .collect();
    dirs.sort();
    let only = std::env::var("SCRIBE_GOLDEN_ONLY").ok();
    dirs.retain(|d| only.as_deref().is_none_or(|o| d.to_string_lossy().contains(o)));

    let results: Vec<_> = std::thread::scope(|s| {
        let handles: Vec<_> = dirs.iter().map(|d| s.spawn(move || check_notebook(d))).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let (mut problems, mut pages, mut strokes, mut points) = (Vec::new(), 0, 0, 0);
    for (p, a, b, c) in results {
        problems.extend(p);
        pages += a;
        strokes += b;
        points += c;
    }
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems.join("\n")
    );
    eprintln!(
        "{} notebooks, {pages} pages, {strokes} strokes, {points} points match the reference harness",
        dirs.len()
    );
}
