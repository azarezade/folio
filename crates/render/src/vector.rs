//! Vector backend: a page as PDF content-stream operators.
//!
//! Coordinates stay in canvas units (integers for stroke points) under one
//! `cm` that the PDF writer supplies. Pressure-shaded strokes become solid grey
//! paths ("vector gray", docs/format.md §4.6). Highlighters go into a separate
//! stream that the writer wraps in a transparency group at 20 % opacity, so
//! overlapping highlights do not darken.

use std::fmt::Write;

use folio_nmdl::{Graphic, Page, PathOp, ShapeGeometry, Stroke, Template};

use crate::geom::{Matrix, argb_gray, is_highlighter, shape_matrix, stroke_matrix, stroke_paths};

/// Resource names the page streams use; the PDF writer defines them.
pub const TEMPLATE_XOBJECT: &str = "Tpl";
pub const HIGHLIGHTER_XOBJECT: &str = "Hl";
pub const HIGHLIGHTER_GSTATE: &str = "GsHl";

#[derive(Clone, Debug, Default)]
pub struct VectorPage {
    /// Drawn in canvas space (the writer maps canvas units to points).
    pub content: Vec<u8>,
    /// Highlighter strokes, for a transparency group drawn where kfxlib puts it.
    pub highlighter: Option<Vec<u8>>,
    /// The content draws the template form `/Tpl`.
    pub uses_template: bool,
}

/// Formats a number compactly (PDF has no exponent notation).
fn num(out: &mut String, v: f64) {
    if v.fract() == 0.0 && v.abs() < 1e12 {
        let _ = write!(out, "{}", v as i64);
    } else {
        let s = format!("{v:.3}");
        out.push_str(s.trim_end_matches('0').trim_end_matches('.'));
    }
}

fn gray_op(out: &mut String, gray: u8, op: &str) {
    num(out, f64::from(gray) / 255.0);
    let _ = writeln!(out, " {op}");
}

fn matrix_op(out: &mut String, m: Matrix) {
    for v in m {
        num(out, v);
        out.push(' ');
    }
    out.push_str("cm\n");
}

fn strokes(out: &mut String, list: &[Stroke], pick: impl Fn(bool) -> bool) {
    let mut current: Option<(u8, f64)> = None;
    for s in list.iter().filter(|s| pick(is_highlighter(s))) {
        let paths = stroke_paths(s);
        if paths.is_empty() {
            continue;
        }
        let transform = stroke_matrix(s);
        if let Some(m) = transform {
            out.push_str("q\n");
            matrix_op(out, m);
        }
        for p in &paths {
            if current.map(|c| c.0) != Some(p.gray) {
                gray_op(out, p.gray, "G");
            }
            if current.map(|c| c.1) != Some(p.width) {
                num(out, p.width);
                out.push_str(" w\n");
            }
            current = Some((p.gray, p.width));
            for (i, (x, y)) in p.points.iter().enumerate() {
                let _ = write!(out, "{x} {y} {}", if i == 0 { "m " } else { "l " });
            }
            out.push_str("S\n");
        }
        if transform.is_some() {
            out.push_str("Q\n");
            current = None;
        }
    }
}

/// Path construction operators for a shape's geometry.
fn shape_ops(out: &mut String, geometry: &ShapeGeometry) -> bool {
    let pt = |out: &mut String, x: f64, y: f64| {
        num(out, x);
        out.push(' ');
        num(out, y);
        out.push(' ');
    };
    match geometry {
        ShapeGeometry::Path(ops) => {
            let mut cur = (0.0, 0.0);
            for op in ops {
                match *op {
                    PathOp::Move(x, y) => {
                        pt(out, x, y);
                        out.push_str("m\n");
                        cur = (x, y);
                    }
                    PathOp::Line(x, y) => {
                        pt(out, x, y);
                        out.push_str("l\n");
                        cur = (x, y);
                    }
                    PathOp::Quad(qx, qy, x, y) => {
                        // as a cubic
                        let c1 = (cur.0 + 2.0 / 3.0 * (qx - cur.0), cur.1 + 2.0 / 3.0 * (qy - cur.1));
                        let c2 = (x + 2.0 / 3.0 * (qx - x), y + 2.0 / 3.0 * (qy - y));
                        pt(out, c1.0, c1.1);
                        pt(out, c2.0, c2.1);
                        pt(out, x, y);
                        out.push_str("c\n");
                        cur = (x, y);
                    }
                    PathOp::Cubic(a, b, c, d, x, y) => {
                        pt(out, a, b);
                        pt(out, c, d);
                        pt(out, x, y);
                        out.push_str("c\n");
                        cur = (x, y);
                    }
                    PathOp::Close => out.push_str("h\n"),
                }
            }
            !ops.is_empty()
        }
        ShapeGeometry::Ellipse { cx, cy, rx, ry } => {
            const K: f64 = 0.552_284_749_830_793_4;
            let (cx, cy, rx, ry) = (*cx, *cy, *rx, *ry);
            pt(out, cx + rx, cy);
            out.push_str("m\n");
            for (c1, c2, end) in [
                ((cx + rx, cy + K * ry), (cx + K * rx, cy + ry), (cx, cy + ry)),
                ((cx - K * rx, cy + ry), (cx - rx, cy + K * ry), (cx - rx, cy)),
                ((cx - rx, cy - K * ry), (cx - K * rx, cy - ry), (cx, cy - ry)),
                ((cx + K * rx, cy - ry), (cx + rx, cy - K * ry), (cx + rx, cy)),
            ] {
                pt(out, c1.0, c1.1);
                pt(out, c2.0, c2.1);
                pt(out, end.0, end.1);
                out.push_str("c\n");
            }
            out.push_str("h\n");
            true
        }
        ShapeGeometry::Rectangle { x, y, width, height } => {
            pt(out, *x, *y);
            pt(out, *width, *height);
            out.push_str("re\n");
            true
        }
        ShapeGeometry::Polyline(v) | ShapeGeometry::Polygon(v) => {
            for (i, p) in v.as_chunks::<2>().0.iter().enumerate() {
                pt(out, p[0], p[1]);
                out.push_str(if i == 0 { "m\n" } else { "l\n" });
            }
            if matches!(geometry, ShapeGeometry::Polygon(_)) {
                out.push_str("h\n");
            }
            v.len() >= 4
        }
        ShapeGeometry::Unknown => false,
    }
}

fn graphics(out: &mut String, graphics: &[Graphic]) {
    for g in graphics {
        for s in &g.shapes {
            out.push_str("q\n");
            matrix_op(out, shape_matrix(g, s));
            if let Some(f) = s.fill {
                gray_op(out, argb_gray(f).0, "g");
            }
            if let Some(st) = s.stroke {
                gray_op(out, argb_gray(st).0, "G");
                num(out, s.stroke_width.unwrap_or(1.0));
                out.push_str(" w 0 J 0 j\n");
            }
            if shape_ops(out, &s.geometry) {
                out.push_str(match (s.fill.is_some(), s.stroke.is_some()) {
                    (true, true) => "B\n",
                    (true, false) => "f\n",
                    (false, true) => "S\n",
                    (false, false) => "n\n",
                });
            }
            out.push_str("Q\n");
        }
    }
}

/// A template's content, in its own coordinates (e.g. 1860×2480).
pub fn template_content(template: &Template) -> Vec<u8> {
    let mut out = String::new();
    graphics(&mut out, &template.graphics);
    out.into_bytes()
}

/// A page's content in canvas units. `template` is the template's size if the
/// page has one (it is stretched over the canvas).
pub fn vector_page(page: &Page, template: Option<(f64, f64)>, shapes: bool) -> VectorPage {
    let mut out = String::new();
    let uses_template = template.is_some();
    if let Some((tw, th)) = template {
        out.push_str("q\n");
        matrix_op(
            &mut out,
            [page.canvas.0 as f64 / tw, 0.0, 0.0, page.canvas.1 as f64 / th, 0.0, 0.0],
        );
        let _ = writeln!(out, "/{TEMPLATE_XOBJECT} Do\nQ");
    }
    out.push_str("1 J 1 j\n");

    let first_hl = page.strokes.iter().position(is_highlighter);
    let mut highlighter = None;
    match first_hl {
        None => strokes(&mut out, &page.strokes, |_| true),
        Some(split) => {
            strokes(&mut out, &page.strokes[..split], |hl| !hl);
            let mut hl = String::from("1 J 1 j\n");
            strokes(&mut hl, &page.strokes, |hl| hl);
            highlighter = Some(hl.into_bytes());
            let _ = writeln!(out, "q /{HIGHLIGHTER_GSTATE} gs /{HIGHLIGHTER_XOBJECT} Do Q");
            // the group's state is restored; set the stroke style again
            out.push_str("1 J 1 j\n");
            strokes(&mut out, &page.strokes[split..], |hl| !hl);
        }
    }
    if shapes {
        graphics(&mut out, &page.graphics);
    }
    VectorPage {
        content: out.into_bytes(),
        highlighter,
        uses_template,
    }
}
