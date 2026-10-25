//! Pages and strokes, decoded lazily one page at a time.
//!
//! Port of `process_scribe_notebook_page_section`, `process_notebook_content`
//! and `scribe_notebook_stroke` in `kfxlib/yj_to_epub_notebook.py`, minus the
//! SVG output (docs/format.md §3–4). kfxlib strips all annotations before this
//! stage (`replace_ion_data`), after resolving `$598::` kfxids to names; the
//! readers here do the same with [`plain`] and [`Kdf::ref_name`].

use std::path::Path;

use folio_kdf::ion::{Diagnostics, Struct, SymbolId, Value};
use folio_kdf::{Kdf, OpenOptions, sym};
use serde::Serialize;

use crate::codec::decode_stroke_values;
use crate::{Outline, Result, SectionKind, outline};

const MAX_DEPTH: usize = 256;

/// `$156` layout value kfxlib accepts on notebook containers.
const LAYOUT_SCALE_FIT: SymbolId = 323;
const LOCATION_ID: SymbolId = 155;
const LAYOUT: SymbolId = 156;
const TRANSFORM: SymbolId = 98;
const GRAPHIC: SymbolId = 272;
const PATH_SHAPE: SymbolId = 273;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Brush {
    OriginalPen,
    Highlighter,
    Pencil,
    FountainPen,
    Marker,
    Pen,
    Unknown(i64),
}

impl Brush {
    /// kfxlib's brush mapping; type 7 is a marker only when its thickness varies.
    pub fn from_type(brush_type: i64, variable_thickness: bool) -> Brush {
        match brush_type {
            0 => Brush::OriginalPen,
            1 => Brush::Highlighter,
            5 => Brush::Pencil,
            6 => Brush::FountainPen,
            7 if variable_thickness => Brush::Marker,
            7 => Brush::Pen,
            n => Brush::Unknown(n),
        }
    }

    /// The name kfxlib writes in each stroke's `<desc>`.
    pub fn name(&self) -> String {
        match self {
            Brush::OriginalPen => "original pen".into(),
            Brush::Highlighter => "highlighter".into(),
            Brush::Pencil => "pencil".into(),
            Brush::FountainPen => "fountain pen".into(),
            Brush::Marker => "marker".into(),
            Brush::Pen => "pen".into(),
            Brush::Unknown(n) => format!("unknown{n}"),
        }
    }

    /// Highlighter strokes are drawn at 20 % opacity.
    pub fn opacity(&self) -> f64 {
        if *self == Brush::Highlighter { 0.2 } else { 1.0 }
    }
}

/// The per-point arrays of a stroke, as decoded (relative to the stroke bounds).
/// `None` when the array is absent or could not be decoded.
#[derive(Clone, Debug, Default, Serialize)]
pub struct PointArrays {
    pub position_x: Option<Vec<i64>>,
    pub position_y: Option<Vec<i64>>,
    pub density_adjust_factor: Option<Vec<i64>>,
    pub thickness_adjust_factor: Option<Vec<i64>>,
    pub tilt_x: Option<Vec<i64>>,
    pub tilt_y: Option<Vec<i64>>,
    pub pressure: Option<Vec<i64>>,
}

pub const ARRAY_NAMES: [&str; 7] = [
    "position_x",
    "position_y",
    "density_adjust_factor",
    "thickness_adjust_factor",
    "tilt_x",
    "tilt_y",
    "pressure",
];

impl PointArrays {
    pub fn get(&self, name: &str) -> Option<&Vec<i64>> {
        match name {
            "position_x" => self.position_x.as_ref(),
            "position_y" => self.position_y.as_ref(),
            "density_adjust_factor" => self.density_adjust_factor.as_ref(),
            "thickness_adjust_factor" => self.thickness_adjust_factor.as_ref(),
            "tilt_x" => self.tilt_x.as_ref(),
            "tilt_y" => self.tilt_y.as_ref(),
            "pressure" => self.pressure.as_ref(),
            _ => None,
        }
    }

    fn slot(&mut self, name: &str) -> &mut Option<Vec<i64>> {
        match name {
            "position_x" => &mut self.position_x,
            "position_y" => &mut self.position_y,
            "density_adjust_factor" => &mut self.density_adjust_factor,
            "thickness_adjust_factor" => &mut self.thickness_adjust_factor,
            "tilt_x" => &mut self.tilt_x,
            "tilt_y" => &mut self.tilt_y,
            _ => &mut self.pressure,
        }
    }
}

/// A stroke point in canvas units. `t` is the width (thickness × quantised
/// thickness factor), `d` the density factor (1.0 = normal).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Point {
    pub x: i64,
    pub y: i64,
    pub t: i64,
    pub d: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Stroke {
    /// Location id (`$155`, else `$598`).
    pub id: Option<String>,
    pub brush_type: Option<i64>,
    pub brush: Brush,
    /// RGB; 0 is black.
    pub color: Option<i64>,
    /// Nominal width in canvas units.
    pub thickness: f64,
    pub seed: Option<i64>,
    /// `[x0, y0, x1, y1]` in canvas units.
    pub bounds: [i64; 4],
    pub num_points: usize,
    pub variable_density: bool,
    pub variable_thickness: bool,
    /// SVG-style affine matrix `[a, b, c, d, e, f]`, if the stroke was moved or scaled.
    pub transform: Option<Vec<f64>>,
    /// Fields kfxlib does not interpret (e.g. `nmdl.origin_stroke_id`), as text.
    pub extra_fields: Vec<(String, String)>,
    /// Unknown keys in `nmdl.stroke_points`.
    pub extra_point_fields: Vec<String>,
    pub arrays: PointArrays,
    /// Built points (docs/format.md §4.3): absolute, consecutive duplicates removed.
    pub points: Vec<Point>,
}

/// A path command, in the graphic's own coordinates.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum PathOp {
    Move(f64, f64),
    Line(f64, f64),
    Quad(f64, f64, f64, f64),
    Cubic(f64, f64, f64, f64, f64, f64),
    Close,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeGeometry {
    /// `$273` paths and `line` shapes.
    Path(Vec<PathOp>),
    Ellipse {
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
    },
    Rectangle {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    /// Arrow heads and similar: vertices as `[x0, y0, x1, y1, …]`.
    Polyline(Vec<f64>),
    Polygon(Vec<f64>),
    Unknown,
}

/// One shape of a [`Graphic`]. Colours are ARGB.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Shape {
    /// `$159`: `$273` (path), `line`, `ellipse`, `rectangle`, `polyline`, `polygon`.
    pub kind: String,
    pub geometry: ShapeGeometry,
    pub stroke: Option<u32>,
    pub fill: Option<u32>,
    pub fill_opacity: Option<f64>,
    pub stroke_width: Option<f64>,
    pub transform: Option<Vec<f64>>,
}

/// A `$272` vector graphic: shapes the Scribe made with "convert to shape".
/// kfxlib skips these on notebook pages (docs/format.md §7), so the Python app
/// never drew them.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Graphic {
    pub id: Option<String>,
    /// Top-left corner on the page canvas (`$59` left, `$58` top).
    pub left: f64,
    pub top: f64,
    /// Box size (`$56`/`$57`, else `$66`/`$67`).
    pub width: f64,
    pub height: f64,
    /// Rotation etc. of the whole graphic (`$98`), about `origin` (`$549`).
    pub transform: Option<Vec<f64>>,
    pub origin: Option<(f64, f64)>,
    pub shapes: Vec<Shape>,
}

/// A page template (dotted, ruled, …), drawn under a page's strokes and
/// stretched over its canvas. Templates are generic YJ content; on the Scribe
/// they are one `$272` graphic in a `width`×`height` box (1860×2480).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Template {
    pub section: String,
    pub template_type: Option<String>,
    pub width: f64,
    pub height: f64,
    pub graphics: Vec<Graphic>,
}

/// Content that is neither a container nor a graphic.
#[derive(Clone, Debug, Serialize)]
pub struct UnhandledContent {
    pub content_type: String,
    pub id: Option<String>,
    /// The node as Ion text, truncated.
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Page {
    /// 1-based position in the notebook.
    pub index: usize,
    pub section: String,
    pub canvas: (i64, i64),
    pub normalized_ppi: Option<f64>,
    /// The page's template section, or the notebook default.
    pub template_id: Option<String>,
    pub template_id_explicit: bool,
    pub inline_placement_type: Option<String>,
    pub stroke_groups: usize,
    pub strokes: Vec<Stroke>,
    pub graphics: Vec<Graphic>,
    pub unhandled: Vec<UnhandledContent>,
    /// Annotation notebooks (`!!PDOC!!`): where the canvas sits on the
    /// document page. `None` for ordinary notebook pages.
    pub placement: Option<Placement>,
}

/// The canvas box relative to the document page, as fractions of the page's
/// width and height (`$59` left, `$58` top, `$56` width, `$57` height, all in
/// percent). A PDF page narrower than 3:4 gets a wider canvas, centred: e.g.
/// left −0.03125, width 1.0625 for 420×595 pt.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Placement {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// The `nmdl.*` local symbols. Notebooks define them in their own symbol
/// table, so ids differ between files.
#[derive(Clone)]
struct Syms {
    ids: std::collections::HashMap<&'static str, SymbolId>,
}

const NMDL_NAMES: [&str; 26] = [
    "nmdl.canvas_width",
    "nmdl.canvas_height",
    "nmdl.normalized_ppi",
    "nmdl.template_id",
    "nmdl.inline_placement_type",
    "nmdl.type",
    "nmdl.chunked",
    "nmdl.chunk_threshold",
    "nmdl.brush_type",
    "nmdl.color",
    "nmdl.random_seed",
    "nmdl.stroke_bounds",
    "nmdl.thickness",
    "nmdl.stroke_points",
    "nmdl.num_points",
    "nmdl.position_x",
    "nmdl.position_y",
    "nmdl.density_adjust_factor",
    "nmdl.thickness_adjust_factor",
    "nmdl.tilt_x",
    "nmdl.tilt_y",
    "nmdl.pressure",
    "nmdl.stroke",
    "nmdl.stroke_group",
    "nmdl.template_type",
    "nmdl.origin_stroke_id",
];

impl Syms {
    fn new(kdf: &Kdf) -> Syms {
        let ids = NMDL_NAMES
            .iter()
            .filter_map(|&n| Some((n, kdf.symtab().id(n)?)))
            .collect();
        Syms { ids }
    }

    fn get(&self, name: &str) -> Option<SymbolId> {
        self.ids.get(name).copied()
    }

    fn field<'v>(&self, st: &Struct<'v>, name: &str) -> Result<Option<Value<'v>>> {
        Ok(match self.get(name) {
            Some(id) => st.get(id)?,
            None => None,
        })
    }
}

/// The value with annotations removed, as kfxlib's `replace_ion_data` leaves it
/// (except `$598::` kfxids, which callers resolve with [`Kdf::ref_name`]).
pub fn plain(value: Value<'_>) -> Value<'_> {
    match value {
        Value::Annotated(a) if a.first_annotation() != Ok(sym::KFX_ID) => a.value().map(plain).unwrap_or(value),
        v => v,
    }
}

/// A value as kfxlib's notebook code would print it: names for symbols and
/// kfxids, the text of strings, Ion text otherwise.
fn plain_text(kdf: &Kdf, value: Value<'_>) -> String {
    if let Some(name) = kdf.ref_name(value) {
        return name.into_owned();
    }
    match plain(value) {
        Value::String(s) => s.to_string(),
        v => folio_kdf::ion::text::to_text(v, kdf.symtab(), Default::default()),
    }
}

fn int(v: Option<Value<'_>>) -> Option<i64> {
    v.map(plain).and_then(|v| v.as_i64())
}

/// `float()` of an Ion int, float or decimal, rounded exactly as Python does.
fn float(v: Value<'_>) -> Option<f64> {
    match plain(v) {
        Value::Decimal(d) => format!("{}e{}", d.coefficient, d.exponent).parse().ok(),
        v => v.as_f64(),
    }
}

pub struct Notebook {
    kdf: Kdf,
    outline: Outline,
    syms: Syms,
    default_template_id: Option<String>,
    pages: Vec<usize>,
}

impl Notebook {
    pub fn open(path: impl AsRef<Path>, options: &OpenOptions, diag: &mut Diagnostics) -> Result<Notebook> {
        let kdf = Kdf::open(path, options)?;
        Notebook::from_kdf(kdf, diag)
    }

    pub fn from_kdf(kdf: Kdf, diag: &mut Diagnostics) -> Result<Notebook> {
        diag.merge(kdf.diagnostics().clone());
        let outline = outline(&kdf, diag)?;
        let syms = Syms::new(&kdf);
        let mut default_template_id = None;
        if let Some(doc) = kdf.first_of(sym::DOCUMENT_DATA) {
            let data = kdf.read(doc)?;
            if let Some(st) = data.value()?.as_struct()
                && let Some(v) = syms.field(&st, "nmdl.template_id")?
            {
                default_template_id = Some(plain_text(&kdf, v));
            }
        }
        let pages = outline
            .sections
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind == SectionKind::Page)
            .map(|(i, _)| i)
            .collect();
        Ok(Notebook {
            kdf,
            outline,
            syms,
            default_template_id,
            pages,
        })
    }

    pub fn kdf(&self) -> &Kdf {
        &self.kdf
    }

    /// Another handle with its own database connection, for another thread.
    pub fn try_clone(&self) -> Result<Notebook> {
        Ok(Notebook {
            kdf: self.kdf.try_clone()?,
            outline: self.outline.clone(),
            syms: self.syms.clone(),
            default_template_id: self.default_template_id.clone(),
            pages: self.pages.clone(),
        })
    }

    pub fn into_kdf(self) -> Kdf {
        self.kdf
    }

    pub fn outline(&self) -> &Outline {
        &self.outline
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// The notebook's default template (`nmdl.template_id` in `document_data`).
    pub fn default_template_id(&self) -> Option<&str> {
        self.default_template_id.as_deref()
    }

    /// The template section `name` (a page's `template_id`), if the notebook has it.
    pub fn template(&self, name: &str, diag: &mut Diagnostics) -> Result<Option<Template>> {
        let Some(info) = self.outline.templates().find(|t| t.name == name) else {
            return Ok(None);
        };
        let Some(frag) = self.kdf.get(sym::SECTION, Some(name)) else {
            return Ok(None);
        };
        let data = self.kdf.read(frag)?;
        let mut graphics = Vec::new();
        if let Some(st) = data.value()?.as_struct()
            && let Some(templates) = st.get(sym::PAGE_TEMPLATES)?.and_then(|v| plain(v).as_seq())
            && let Some(first) = templates.iter().next()
        {
            self.collect_graphics(first?, &mut graphics, diag, 0)?;
        }
        if graphics.is_empty() {
            let t = info.template_type.clone().unwrap_or_default();
            diag.warning("nmdl.template_empty", || {
                format!("template {t} has no drawable content; drawn blank")
            });
        }
        let (width, height) = graphics
            .iter()
            .map(|g| (g.left + g.width, g.top + g.height))
            .fold((0f64, 0f64), |(w, h), (gw, gh)| (w.max(gw), h.max(gh)));
        let (width, height) = if width > 0.0 && height > 0.0 {
            (width, height)
        } else {
            (1860.0, 2480.0)
        };
        Ok(Some(Template {
            section: name.into(),
            template_type: info.template_type.clone(),
            width,
            height,
            graphics,
        }))
    }

    /// Finds `$272` graphics anywhere under `value` (templates nest them in `$269` stories).
    fn collect_graphics(
        &self,
        value: Value<'_>,
        out: &mut Vec<Graphic>,
        diag: &mut Diagnostics,
        depth: usize,
    ) -> Result<()> {
        if depth > MAX_DEPTH {
            return Ok(());
        }
        if let Some(name) = self.kdf.ref_name(value) {
            if let Some(frag) = self.kdf.get(sym::STRUCTURE, Some(&name)) {
                let data = self.kdf.read(frag)?;
                self.collect_graphics(data.value()?, out, diag, depth + 1)?;
            }
            return Ok(());
        }
        let Some(st) = plain(value).as_struct() else {
            return Ok(());
        };
        if st.get(sym::CONTENT_TYPE)?.and_then(|v| plain(v).as_symbol()) == Some(GRAPHIC) {
            out.push(self.graphic(&st, None, diag)?);
            return Ok(());
        }
        if let Some(children) = st.get(sym::STORY_CONTENT)?.and_then(|v| plain(v).as_seq()) {
            for c in children {
                self.collect_graphics(c?, out, diag, depth + 1)?;
            }
        } else if let Some(story) = st.get(sym::STORY_NAME)?
            && let Some(name) = self.kdf.ref_name(story)
            && let Some(frag) = self.kdf.get(sym::STORY, Some(&name))
        {
            let data = self.kdf.read(frag)?;
            if let Some(s) = data.value()?.as_struct()
                && let Some(children) = s.get(sym::STORY_CONTENT)?.and_then(|v| plain(v).as_seq())
            {
                for c in children {
                    self.collect_graphics(c?, out, diag, depth + 1)?;
                }
            }
        }
        Ok(())
    }

    /// Decodes page `index` (0-based).
    pub fn page(&self, index: usize, diag: &mut Diagnostics) -> Result<Page> {
        let section = &self.outline.sections[self.pages[index]];
        let frag = self
            .kdf
            .get(sym::SECTION, Some(&section.name))
            .ok_or_else(|| folio_kdf::Error::Invalid(format!("section {} is missing", section.name)))?;
        let data = self.kdf.read(frag)?;
        let st = data
            .value()?
            .as_struct()
            .ok_or_else(|| folio_kdf::Error::Invalid(format!("section {} is not a struct", section.name)))?;

        let canvas = (
            int(self.syms.field(&st, "nmdl.canvas_width")?).unwrap_or(0),
            int(self.syms.field(&st, "nmdl.canvas_height")?).unwrap_or(0),
        );
        if !known_canvas(canvas) {
            diag.warning("nmdl.canvas", || format!("unexpected canvas {}×{}", canvas.0, canvas.1));
        }
        let normalized_ppi = self.syms.field(&st, "nmdl.normalized_ppi")?.and_then(float);
        if normalized_ppi != Some(2520.0) {
            diag.error("nmdl.ppi", || {
                format!("unexpected nmdl.normalized_ppi {normalized_ppi:?}")
            });
        }
        let explicit = self.syms.field(&st, "nmdl.template_id")?;
        let template_id = match explicit {
            Some(v) => Some(plain_text(&self.kdf, v)),
            None => self.default_template_id.clone(),
        };
        let inline_placement_type = self
            .syms
            .field(&st, "nmdl.inline_placement_type")?
            .map(|v| plain_text(&self.kdf, v));

        let percent = |k: SymbolId| -> Result<Option<f64>> { Ok(length(st.get(k)?).map(|v| v / 100.0)) };
        let placement = match (percent(56)?, percent(57)?) {
            (Some(width), Some(height)) => Some(Placement {
                left: percent(59)?.unwrap_or(0.0),
                top: percent(58)?.unwrap_or(0.0),
                width,
                height,
            }),
            _ => None,
        };

        let mut page = Page {
            index: index + 1,
            section: section.name.clone(),
            canvas,
            normalized_ppi,
            template_id,
            template_id_explicit: explicit.is_some(),
            inline_placement_type,
            stroke_groups: 0,
            strokes: Vec::new(),
            graphics: Vec::new(),
            unhandled: Vec::new(),
            placement,
        };

        let template = st
            .get(sym::PAGE_TEMPLATES)?
            .and_then(|v| plain(v).as_seq())
            .and_then(|s| s.iter().next());
        match template {
            Some(t) => self.content(t?, &mut page, diag, 0)?,
            None => diag.error("nmdl.no_page_template", || {
                format!("section {} has no page template", section.name)
            }),
        }
        Ok(page)
    }

    /// `process_notebook_content`: walks containers and stories down to strokes.
    fn content(&self, value: Value<'_>, page: &mut Page, diag: &mut Diagnostics, depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(folio_kdf::Error::Invalid("notebook content nested too deeply".into()));
        }
        // A reference to a $608 structure fragment
        if let Some(name) = self.kdf.ref_name(value) {
            let Some(frag) = self.kdf.get(sym::STRUCTURE, Some(&name)) else {
                diag.error("nmdl.missing_structure", || {
                    format!("missing structure fragment {name}")
                });
                return Ok(());
            };
            let data = self.kdf.read(frag)?;
            return self.content(data.value()?, page, diag, depth + 1);
        }
        let Some(st) = plain(value).as_struct() else {
            diag.error("nmdl.content_type", || {
                format!("content is {:?}, not a struct", value.ion_type())
            });
            return Ok(());
        };

        let content_type = st.get(sym::CONTENT_TYPE)?.and_then(|v| plain(v).as_symbol());
        let location = match st.get(LOCATION_ID)? {
            Some(v) => Some((LOCATION_ID, v)),
            None => st.get(sym::KFX_ID)?.map(|v| (sym::KFX_ID, v)),
        };
        let id = location.and_then(|(_, v)| self.kdf.ref_name(v).map(|c| c.into_owned()));

        if content_type == Some(GRAPHIC) {
            diag.debug("nmdl.graphic", || "page has vector graphics (kfxlib skips them)".into());
            let graphic = self.graphic(&st, id, diag)?;
            page.graphics.push(graphic);
            return Ok(());
        }
        if content_type != Some(sym::CONTAINER_CONTENT) {
            let content_type = content_type.map_or("none".into(), |t| self.kdf.symtab().name(t).into_owned());
            diag.debug("nmdl.unhandled_content", || {
                format!("skipped content of type {content_type}, as kfxlib does")
            });
            let text = folio_kdf::ion::text::to_text(
                plain(value),
                self.kdf.symtab(),
                folio_kdf::ion::text::TextOptions {
                    max_depth: 8,
                    max_items: 40,
                    max_blob: 16,
                },
            );
            page.unhandled.push(UnhandledContent {
                content_type,
                id,
                text: truncate(text, 2000),
            });
            return Ok(());
        }

        let layout = st.get(LAYOUT)?.and_then(|v| plain(v).as_symbol());
        let mut used = vec![sym::CONTENT_TYPE, LAYOUT];
        used.extend(location.map(|(k, _)| k));

        if let Some(children) = st.get(sym::STORY_CONTENT)? {
            used.push(sym::STORY_CONTENT);
            if let Some(seq) = plain(children).as_seq() {
                for child in seq {
                    self.content(child?, page, diag, depth + 1)?;
                }
            }
        } else if let Some(story) = st.get(sym::STORY_NAME)? {
            used.push(sym::STORY_NAME);
            let name = self.kdf.ref_name(story).map(|c| c.into_owned()).unwrap_or_default();
            match self.kdf.get(sym::STORY, Some(&name)) {
                Some(frag) => {
                    let data = self.kdf.read(frag)?;
                    if let Some(story) = data.value()?.as_struct()
                        && let Some(seq) = story.get(sym::STORY_CONTENT)?.and_then(|v| plain(v).as_seq())
                    {
                        for child in seq {
                            self.content(child?, page, diag, depth + 1)?;
                        }
                    }
                }
                None => diag.error("nmdl.missing_story", || format!("missing story fragment {name}")),
            }
        }

        match layout {
            None => {
                if let Some(t) = self.syms.field(&st, "nmdl.type")? {
                    self.stroke_node(&st, t, id, &used, page, diag)?;
                }
            }
            Some(LAYOUT_SCALE_FIT) => {}
            Some(other) => {
                let name = self.kdf.symtab().name(other).into_owned();
                diag.error("nmdl.layout", || format!("unknown container layout {name}"));
            }
        }
        Ok(())
    }

    fn graphic(&self, st: &Struct<'_>, id: Option<String>, diag: &mut Diagnostics) -> Result<Graphic> {
        let f = |k: SymbolId| st.get(k);
        let origin = match f(549)?.and_then(|v| plain(v).as_struct()) {
            // `$59` is left and `$58` is top (kfxlib yj_to_epub_properties)
            Some(o) => Some((length(o.get(59)?).unwrap_or(0.0), length(o.get(58)?).unwrap_or(0.0))),
            None => None,
        };
        let mut shapes = Vec::new();
        if let Some(list) = f(250)?.and_then(|v| plain(v).as_seq()) {
            for shape in list {
                if let Some(sh) = plain(shape?).as_struct() {
                    shapes.push(self.shape(&sh, diag)?);
                }
            }
        }
        for field in st.fields() {
            let (k, _) = field?;
            if ![
                sym::CONTENT_TYPE,
                sym::KFX_ID,
                LOCATION_ID,
                66,
                67,
                56,
                57,
                58,
                59,
                183,
                250,
                TRANSFORM,
                549,
            ]
            .contains(&k)
            {
                let key = self.kdf.symtab().name(k).into_owned();
                diag.warning("graphic.field", || format!("vector graphic has unknown field {key}"));
            }
        }
        Ok(Graphic {
            id,
            left: length(f(59)?).unwrap_or(0.0),
            top: length(f(58)?).unwrap_or(0.0),
            width: length(f(56)?).or(length(f(66)?)).unwrap_or(0.0),
            height: length(f(57)?).or(length(f(67)?)).unwrap_or(0.0),
            transform: floats(f(TRANSFORM)?),
            origin,
            shapes,
        })
    }

    fn shape(&self, sh: &Struct<'_>, diag: &mut Diagnostics) -> Result<Shape> {
        let symtab = self.kdf.symtab();
        let kind = match sh.get(sym::CONTENT_TYPE)?.map(plain) {
            Some(Value::Symbol(PATH_SHAPE)) => "$273".to_string(),
            Some(v) => plain_text(&self.kdf, v),
            None => "none".into(),
        };
        let dims = match symtab.id("shape_dimensions") {
            Some(id) => sh.get(id)?.and_then(|v| plain(v).as_struct()),
            None => None,
        };
        let dim = |name: &str| -> Result<Option<f64>> {
            let Some(d) = dims else { return Ok(None) };
            let id = symtab.id(name);
            Ok(match id {
                Some(id) => length(d.get(id)?),
                None => None,
            })
        };
        let vertices = |d: Option<Struct<'_>>| -> Result<Vec<f64>> {
            Ok(match (d, symtab.id("vertex_list")) {
                (Some(d), Some(id)) => floats(d.get(id)?).unwrap_or_default(),
                _ => Vec::new(),
            })
        };
        let geometry = match kind.as_str() {
            "$273" | "line" => ShapeGeometry::Path(path_ops(&floats(sh.get(249)?).unwrap_or_default(), diag)),
            "ellipse" => ShapeGeometry::Ellipse {
                cx: dim("cx")?.unwrap_or(0.0),
                cy: dim("cy")?.unwrap_or(0.0),
                rx: dim("radius_x")?.unwrap_or(0.0),
                ry: dim("radius_y")?.unwrap_or(0.0),
            },
            "rectangle" => ShapeGeometry::Rectangle {
                x: dim("x")?.unwrap_or(0.0),
                y: dim("y")?.unwrap_or(0.0),
                width: match dims {
                    Some(d) => length(d.get(56)?).unwrap_or(0.0),
                    None => 0.0,
                },
                height: match dims {
                    Some(d) => length(d.get(57)?).unwrap_or(0.0),
                    None => 0.0,
                },
            },
            "polyline" => ShapeGeometry::Polyline(vertices(dims)?),
            "polygon" => ShapeGeometry::Polygon(vertices(dims)?),
            other => {
                let other = other.to_string();
                diag.warning("graphic.shape_kind", || format!("unknown shape kind {other}"));
                ShapeGeometry::Unknown
            }
        };
        let dims_id = symtab.id("shape_dimensions");
        for field in sh.fields() {
            let (k, _) = field?;
            if ![sym::CONTENT_TYPE, 249, 70, 72, 75, 76, 498, TRANSFORM].contains(&k) && Some(k) != dims_id {
                let key = symtab.name(k).into_owned();
                diag.warning("graphic.shape_field", || format!("shape has unknown field {key}"));
            }
        }
        Ok(Shape {
            kind,
            geometry,
            stroke: argb(sh.get(75)?),
            fill: argb(sh.get(70)?),
            fill_opacity: sh.get(72)?.and_then(float),
            stroke_width: length(sh.get(76)?),
            transform: floats(sh.get(TRANSFORM)?),
        })
    }

    /// `scribe_notebook_stroke`.
    fn stroke_node(
        &self,
        st: &Struct<'_>,
        nmdl_type: Value<'_>,
        id: Option<String>,
        used: &[SymbolId],
        page: &mut Page,
        diag: &mut Diagnostics,
    ) -> Result<()> {
        let symtab = self.kdf.symtab();
        let type_name = plain_text(&self.kdf, nmdl_type);
        match type_name.as_str() {
            "nmdl.stroke_group" => {
                page.stroke_groups += 1;
                if self
                    .syms
                    .field(st, "nmdl.chunked")?
                    .map(plain)
                    .and_then(|v| v.as_bool())
                    != Some(true)
                {
                    diag.error("nmdl.chunked", || "stroke group without nmdl.chunked = true".into());
                }
                if int(self.syms.field(st, "nmdl.chunk_threshold")?) != Some(50) {
                    diag.error("nmdl.chunk_threshold", || {
                        "stroke group with nmdl.chunk_threshold ≠ 50".into()
                    });
                }
                Ok(())
            }
            "nmdl.stroke" => {
                let stroke = self.stroke(st, id, used, diag)?;
                page.strokes.push(stroke);
                Ok(())
            }
            other => {
                let other = other.to_string();
                diag.error("nmdl.type", || format!("unknown nmdl.type {other}"));
                let _ = symtab;
                Ok(())
            }
        }
    }

    fn stroke(&self, st: &Struct<'_>, id: Option<String>, used: &[SymbolId], diag: &mut Diagnostics) -> Result<Stroke> {
        let s = &self.syms;
        let brush_type = int(s.field(st, "nmdl.brush_type")?);
        let color = int(s.field(st, "nmdl.color")?);
        let seed = int(s.field(st, "nmdl.random_seed")?);
        let thickness = s.field(st, "nmdl.thickness")?.and_then(float).unwrap_or_else(|| {
            diag.error("stroke.thickness", || "stroke has no nmdl.thickness".into());
            0.0
        });
        let mut bounds = [0i64; 4];
        if let Some(b) = s.field(st, "nmdl.stroke_bounds")?.and_then(|v| plain(v).as_seq()) {
            for (slot, v) in bounds.iter_mut().zip(b.iter()) {
                *slot = plain(v?).as_i64().unwrap_or(0);
            }
        } else {
            diag.error("stroke.bounds", || "stroke has no nmdl.stroke_bounds".into());
        }
        if color.is_some_and(|c| c != 0) {
            diag.warning("stroke.color", || format!("stroke colour {color:?}"));
        }
        let transform = match st.get(TRANSFORM)? {
            Some(v) => match plain(v).as_seq() {
                Some(seq) => Some(
                    seq.iter()
                        .map(|v| v.map(|v| float(v).unwrap_or(0.0)))
                        .collect::<std::result::Result<Vec<_>, _>>()?,
                ),
                None => None,
            },
            None => None,
        };

        // nmdl.stroke_points
        let mut num_points = 0usize;
        let mut arrays = PointArrays::default();
        let mut extra_point_fields = Vec::new();
        if let Some(sp) = s.field(st, "nmdl.stroke_points")?.and_then(|v| plain(v).as_struct()) {
            num_points = int(s.field(&sp, "nmdl.num_points")?).unwrap_or(0).max(0) as usize;
            for field in sp.fields() {
                let (k, v) = field?;
                let name = self.kdf.symtab().name(k);
                if name == "nmdl.num_points" {
                    continue;
                }
                match name.strip_prefix("nmdl.").filter(|n| ARRAY_NAMES.contains(n)) {
                    Some(short) => {
                        *arrays.slot(short) = match plain(v).as_bytes() {
                            Some(bytes) => decode_stroke_values(bytes, num_points, &name, diag),
                            None => {
                                diag.error("stroke.array_type", || format!("{name} is not a blob"));
                                None
                            }
                        };
                    }
                    None => extra_point_fields.push(name.into_owned()),
                }
            }
            extra_point_fields.sort();
        }

        // Fields kfxlib leaves unread (it logs them as "extra data")
        let known: Vec<SymbolId> = [
            "nmdl.type",
            "nmdl.brush_type",
            "nmdl.color",
            "nmdl.random_seed",
            "nmdl.stroke_bounds",
            "nmdl.thickness",
            "nmdl.stroke_points",
        ]
        .iter()
        .filter_map(|n| s.get(n))
        .chain([TRANSFORM])
        .chain(used.iter().copied())
        .collect();
        let mut extra_fields = Vec::new();
        for field in st.fields() {
            let (k, v) = field?;
            if !known.contains(&k) {
                let key = self.kdf.symtab().name(k).into_owned();
                if key == "nmdl.origin_stroke_id" {
                    diag.debug("stroke.origin_stroke_id", || {
                        "strokes carry nmdl.origin_stroke_id (copied or moved)".into()
                    });
                } else {
                    diag.warning("stroke.extra_field", || format!("stroke has unknown field {key}"));
                }
                extra_fields.push((key, plain_text(&self.kdf, v)));
            }
        }

        let empty = Vec::new();
        let daf = arrays.density_adjust_factor.as_ref().unwrap_or(&empty);
        let taf = arrays.thickness_adjust_factor.as_ref().unwrap_or(&empty);
        let variable_density = daf.iter().any(|&v| v != 100);
        let variable_thickness = taf.iter().any(|&v| v != 100);
        let brush = Brush::from_type(brush_type.unwrap_or(-1), variable_thickness);
        if let Brush::Unknown(n) = brush {
            diag.error("stroke.brush", || format!("unexpected brush type {n}; drawn as a pen"));
        }

        let xs = arrays.position_x.as_ref().unwrap_or(&empty);
        let ys = arrays.position_y.as_ref().unwrap_or(&empty);
        let mut n = num_points.min(xs.len()).min(ys.len());
        if variable_thickness {
            n = n.min(taf.len());
        }
        if variable_density {
            n = n.min(daf.len());
        }
        if n < num_points {
            diag.error("stroke.short_arrays", || {
                format!("stroke has {num_points} points but arrays hold {n}")
            });
        }
        let mut points = Vec::with_capacity(n);
        let mut last = None;
        for i in 0..n {
            let x = xs[i] + bounds[0];
            let y = ys[i] + bounds[1];
            let t_af = if variable_thickness { taf[i] } else { 100 };
            let d_af = if variable_density { daf[i] } else { 100 };
            if x < bounds[0] || x > bounds[2] || y < bounds[1] || y > bounds[3] {
                diag.error("stroke.out_of_bounds", || {
                    format!("point ({x}, {y}) outside bounds {bounds:?}")
                });
            }
            if !(0..=1000).contains(&t_af) {
                diag.error("stroke.taf_range", || {
                    format!("thickness_adjust_factor out of range: {t_af}")
                });
            }
            if !(0..=300).contains(&d_af) {
                diag.error("stroke.daf_range", || {
                    format!("density_adjust_factor out of range: {d_af}")
                });
            }
            let t_af = t_af.div_euclid(10) * 10; // QUANTIZE_THICKNESS
            let t = (thickness * t_af as f64 / 100.0).round_ties_even() as i64;
            let d = d_af as f64 / 100.0;
            if last != Some((x, y)) {
                points.push(Point { x, y, t, d });
            }
            last = Some((x, y));
        }

        Ok(Stroke {
            id,
            brush_type,
            brush,
            color,
            thickness,
            seed,
            bounds,
            num_points,
            variable_density,
            variable_thickness,
            transform,
            extra_fields,
            extra_point_fields,
            arrays,
            points,
        })
    }
}

/// A length that is either a number or `{$307: value, $306: unit}`.
fn length(v: Option<Value<'_>>) -> Option<f64> {
    let v = plain(v?);
    if let Some(st) = v.as_struct() {
        return st.get(sym::VALUE).ok().flatten().and_then(float);
    }
    float(v)
}

fn floats(v: Option<Value<'_>>) -> Option<Vec<f64>> {
    let seq = plain(v?).as_seq()?;
    seq.iter().map(|x| x.ok().and_then(float)).collect()
}

/// Ion ints holding ARGB, possibly as signed 32-bit.
fn argb(v: Option<Value<'_>>) -> Option<u32> {
    int(v).map(|c| c as u32)
}

/// kfxlib's `process_path`: opcodes 0 M, 1 L, 2 Q, 3 C, 4 Z, each followed by its coordinates.
fn path_ops(vals: &[f64], diag: &mut Diagnostics) -> Vec<PathOp> {
    let mut ops = Vec::new();
    let mut i = 0;
    while i < vals.len() {
        let op = vals[i];
        let n = match op as i64 {
            0 | 1 => 2,
            2 => 4,
            3 => 6,
            4 => 0,
            _ => {
                diag.error("graphic.path_op", || format!("unexpected path instruction {op}"));
                break;
            }
        };
        let Some(a) = vals.get(i + 1..i + 1 + n) else {
            diag.error("graphic.path_short", || "incomplete path instruction".into());
            break;
        };
        ops.push(match op as i64 {
            0 => PathOp::Move(a[0], a[1]),
            1 => PathOp::Line(a[0], a[1]),
            2 => PathOp::Quad(a[0], a[1], a[2], a[3]),
            3 => PathOp::Cubic(a[0], a[1], a[2], a[3], a[4], a[5]),
            _ => PathOp::Close,
        });
        i += 1 + n;
    }
    ops
}

fn known_canvas((w, h): (i64, i64)) -> bool {
    matches!((w, h), (15624, 20832) | (13726, 7350)) || [3906, 13734, 3066, 6132, 12264].contains(&w) || h > 15000
}

fn truncate(mut s: String, max: usize) -> String {
    if s.len() > max {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push('…');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_instructions() {
        let mut d = Diagnostics::default();
        let ops = path_ops(&[0.0, 252.0, 159.6, 1.0, 0.0, 0.0, 4.0], &mut d);
        assert_eq!(
            ops,
            vec![PathOp::Move(252.0, 159.6), PathOp::Line(0.0, 0.0), PathOp::Close]
        );
        assert!(d.is_empty());
        let ops = path_ops(&[3.0, 1.0, 2.0], &mut d);
        assert!(ops.is_empty());
        assert_eq!(d.count("graphic.path_short"), 1);
    }

    #[test]
    fn brushes() {
        assert_eq!(Brush::from_type(7, true), Brush::Marker);
        assert_eq!(Brush::from_type(7, false), Brush::Pen);
        assert_eq!(Brush::from_type(9, false).name(), "unknown9");
        assert_eq!(Brush::Highlighter.opacity(), 0.2);
    }
}
