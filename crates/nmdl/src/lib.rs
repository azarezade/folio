//! Scribe notebook model: pages, templates, strokes and the stroke codec.
//!
//! [`outline`] lists the reading orders and classifies each section (as
//! `yj_to_epub_content.py:115` dispatches them); [`Notebook`] decodes pages and
//! strokes on demand; [`codec`] is the stroke point-array codec.

pub mod codec;
mod model;

pub use model::{
    ARRAY_NAMES, Brush, Graphic, Notebook, Page, PathOp, Placement, Point, PointArrays, Shape, ShapeGeometry, Stroke,
    Template, UnhandledContent, plain,
};

use std::collections::HashSet;

use folio_kdf::ion::{Diagnostics, SymbolId, Value};
use folio_kdf::{Kdf, sym};

pub use folio_kdf::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ReadingOrder {
    pub name: Option<String>,
    pub sections: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SectionKind {
    /// Has `nmdl.canvas_width`: a notebook page.
    Page,
    /// Has `nmdl.template_type`: a page template (dotted, ruled, …).
    Template,
    Other,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct SectionInfo {
    pub name: String,
    pub kind: SectionKind,
    pub canvas: Option<(i64, i64)>,
    pub template_type: Option<String>,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Outline {
    pub reading_orders: Vec<ReadingOrder>,
    /// Sections in processing order, each once.
    pub sections: Vec<SectionInfo>,
    /// Page sections in the database but in no reading order (deleted pages).
    pub unreferenced_pages: Vec<String>,
}

impl Outline {
    pub fn pages(&self) -> impl Iterator<Item = &SectionInfo> {
        self.sections.iter().filter(|s| s.kind == SectionKind::Page)
    }

    pub fn templates(&self) -> impl Iterator<Item = &SectionInfo> {
        self.sections.iter().filter(|s| s.kind == SectionKind::Template)
    }
}

/// Local symbol ids for the `nmdl.*` fields this crate reads (absent if the
/// notebook never defines them).
struct NmdlSymbols {
    canvas_width: Option<SymbolId>,
    canvas_height: Option<SymbolId>,
    template_type: Option<SymbolId>,
}

impl NmdlSymbols {
    fn new(kdf: &Kdf) -> Self {
        let id = |n| kdf.symtab().id(n);
        NmdlSymbols {
            canvas_width: id("nmdl.canvas_width"),
            canvas_height: id("nmdl.canvas_height"),
            template_type: id("nmdl.template_type"),
        }
    }
}

fn field<'v>(st: &folio_kdf::ion::Struct<'v>, id: Option<SymbolId>) -> Result<Option<Value<'v>>> {
    Ok(match id {
        Some(id) => st.get(id)?,
        None => None,
    })
}

/// Reads the reading orders from `document_data` (or `metadata` when there is
/// none), as kfxlib does.
pub fn reading_orders(kdf: &Kdf) -> Result<Vec<ReadingOrder>> {
    let holder = kdf.first_of(sym::DOCUMENT_DATA).or_else(|| kdf.first_of(sym::METADATA));
    let Some(holder) = holder else { return Ok(Vec::new()) };
    let data = kdf.read(holder)?;
    let mut out = Vec::new();
    let Some(st) = data.value()?.as_struct() else {
        return Ok(out);
    };
    let Some(orders) = st.get(sym::READING_ORDERS)?.and_then(|v| v.as_seq()) else {
        return Ok(out);
    };
    for ro in orders {
        let Some(ro) = ro?.as_struct() else { continue };
        let name = ro
            .get(sym::READING_ORDER_NAME)?
            .and_then(|v| kdf.ref_name(v))
            .map(Into::into);
        let mut sections = Vec::new();
        if let Some(list) = ro.get(sym::SECTIONS)?.and_then(|v| v.as_seq()) {
            for s in list {
                if let Some(n) = kdf.ref_name(s?) {
                    sections.push(n.into_owned());
                }
            }
        }
        out.push(ReadingOrder { name, sections });
    }
    Ok(out)
}

pub fn outline(kdf: &Kdf, diag: &mut Diagnostics) -> Result<Outline> {
    let nmdl = NmdlSymbols::new(kdf);
    let reading_orders = reading_orders(kdf)?;
    let mut seen = HashSet::new();
    let mut sections = Vec::new();

    for name in reading_orders.iter().flat_map(|ro| &ro.sections) {
        if !seen.insert(name.clone()) {
            diag.error("nmdl.duplicate_section", || {
                format!("duplicate section {name} in reading order")
            });
            continue;
        }
        let Some(frag) = kdf.get(sym::SECTION, Some(name)) else {
            diag.error("nmdl.missing_section", || format!("section {name} is missing"));
            continue;
        };
        let data = kdf.read(frag)?;
        let value = data.value()?;
        let Some(st) = value.as_struct() else {
            diag.error("nmdl.section_type", || format!("section {name} is not a struct"));
            continue;
        };
        let width = field(&st, nmdl.canvas_width)?;
        let template_type = field(&st, nmdl.template_type)?;
        let info = if let Some(w) = width {
            let h = field(&st, nmdl.canvas_height)?.and_then(|v| v.as_i64());
            SectionInfo {
                name: name.clone(),
                kind: SectionKind::Page,
                canvas: w.as_i64().zip(h),
                template_type: None,
            }
        } else if let Some(t) = template_type {
            let t = t
                .as_str()
                .map(Into::into)
                .or_else(|| kdf.ref_name(t).map(|c| c.into_owned()));
            SectionInfo {
                name: name.clone(),
                kind: SectionKind::Template,
                canvas: None,
                template_type: t,
            }
        } else {
            SectionInfo {
                name: name.clone(),
                kind: SectionKind::Other,
                canvas: None,
                template_type: None,
            }
        };
        sections.push(info);
    }

    let mut unreferenced_pages: Vec<String> = kdf
        .fragments()
        .iter()
        .filter(|f| f.ftype() == sym::SECTION)
        .filter_map(|f| f.fid())
        .filter(|fid| !seen.contains(*fid))
        .map(Into::into)
        .collect();
    unreferenced_pages.sort();
    if !unreferenced_pages.is_empty() {
        let list = unreferenced_pages.join(", ");
        diag.debug("nmdl.unreferenced_pages", || {
            format!("notebook contains unreferenced pages: {list}")
        });
    }

    Ok(Outline {
        reading_orders,
        sections,
        unreferenced_pages,
    })
}
