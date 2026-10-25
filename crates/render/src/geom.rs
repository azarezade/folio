//! Geometry shared by the raster and vector backends: how a stroke splits into
//! paths, its colours, and the canvas → output scale.

use folio_nmdl::{Brush, Stroke};

/// kfxlib's `PNG_DENSITY_GAMMA`.
pub const DENSITY_GAMMA: f64 = 3.5;
/// Grey levels for pressure-shaded strokes drawn as paths (`VECTOR_DENSITY_LEVELS`).
pub const DENSITY_LEVELS: f64 = 16.0;

/// Output size in pixels for a canvas, with its long edge at `long_edge`.
pub fn output_size(canvas: (i64, i64), long_edge: u32) -> (u32, u32) {
    let (w, h) = (canvas.0.max(1) as f64, canvas.1.max(1) as f64);
    let k = f64::from(long_edge) / w.max(h);
    (((w * k).round() as u32).max(1), ((h * k).round() as u32).max(1))
}

/// Grey (0 black … 255 white) of an RGB colour: the average, as kfxlib's
/// `adjust_color_for_density` computes luminance.
pub fn rgb_gray(color: i64) -> u8 {
    let c = color as u32;
    (((c >> 16 & 255) + (c >> 8 & 255) + (c & 255)) / 3) as u8
}

/// Grey and alpha of an ARGB colour (as stored in vector graphics).
pub fn argb_gray(argb: u32) -> (u8, f64) {
    (rgb_gray(i64::from(argb & 0xff_ffff)), f64::from(argb >> 24) / 255.0)
}

/// Density after kfxlib's gamma curve: `1 - (1 - d)^3.5`, with `d` clamped to 0..1.
pub fn adjusted_density(d: f64) -> f64 {
    1.0 - (1.0 - d.clamp(0.0, 1.0)).powf(DENSITY_GAMMA)
}

/// `adjust_color_for_density`: the grey a colour fades to at `density`.
pub fn gray_for_density(color: i64, density: f64) -> u8 {
    let lum = i64::from(rgb_gray(color));
    let v = 255 - ((255 - lum) as f64 * density).trunc() as i64;
    v.clamp(0, 255) as u8
}

/// One SVG `<path>` of a stroke, as kfxlib emits it.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokePath {
    pub points: Vec<(i64, i64)>,
    /// Line width in canvas units.
    pub width: f64,
    /// Grey of this path (black for most strokes; lighter for pressure-shaded
    /// strokes drawn as paths).
    pub gray: u8,
}

/// Splits a stroke into paths wherever the width or density changes, each
/// starting from the two previous points so the pieces overlap
/// (`INCLUDE_PRIOR_LINE_SEGMENT`). Single-point paths are dropped, as kfxlib does.
/// Pressure-shaded strokes get quantised grey levels (docs/format.md §4.6).
pub fn stroke_paths(stroke: &Stroke) -> Vec<StrokePath> {
    let color = stroke.color.unwrap_or(0);
    let base_gray = rgb_gray(color);
    let pts: Vec<(i64, i64, i64, f64)> = stroke
        .points
        .iter()
        .map(|p| {
            let d = if stroke.variable_density {
                (adjusted_density(p.d) * DENSITY_LEVELS).round_ties_even() / DENSITY_LEVELS
            } else {
                p.d
            };
            (p.x, p.y, p.t, d)
        })
        .collect();

    // (points, t, d) per path
    type Run = (Vec<(i64, i64)>, i64, f64);
    let mut paths: Vec<Run> = Vec::new();
    let mut prev: Option<(i64, f64)> = None;
    for (i, &(x, y, t, d)) in pts.iter().enumerate() {
        if prev != Some((t, d)) {
            let mut path = Vec::new();
            for j in [2, 1] {
                if i >= j {
                    path.push((pts[i - j].0, pts[i - j].1));
                }
            }
            paths.push((path, t, d));
        }
        paths.last_mut().unwrap().0.push((x, y));
        prev = Some((t, d));
    }

    paths
        .into_iter()
        .filter(|(p, _, _)| p.len() > 1)
        .map(|(points, t, d)| StrokePath {
            points,
            // a path inherits the group's round(thickness) only when t equals it
            width: if t as f64 != stroke.thickness {
                t as f64
            } else {
                stroke.thickness.round_ties_even()
            },
            gray: if stroke.variable_density {
                gray_for_density(color, d)
            } else {
                base_gray
            },
        })
        .collect()
}

pub fn is_highlighter(stroke: &Stroke) -> bool {
    stroke.brush == Brush::Highlighter
}

/// The stroke's own transform as `[a, b, c, d, e, f]`, if it has a valid one.
pub fn stroke_matrix(stroke: &Stroke) -> Option<Matrix> {
    matrix6(&stroke.transform)
}

/// An affine matrix `[a, b, c, d, e, f]` (SVG order): x' = a·x + c·y + e, y' = b·x + d·y + f.
pub type Matrix = [f64; 6];

pub const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `m ∘ n`: apply `n`, then `m`.
pub fn mul(m: Matrix, n: Matrix) -> Matrix {
    [
        m[0] * n[0] + m[2] * n[1],
        m[1] * n[0] + m[3] * n[1],
        m[0] * n[2] + m[2] * n[3],
        m[1] * n[2] + m[3] * n[3],
        m[0] * n[4] + m[2] * n[5] + m[4],
        m[1] * n[4] + m[3] * n[5] + m[5],
    ]
}

pub fn translate(x: f64, y: f64) -> Matrix {
    [1.0, 0.0, 0.0, 1.0, x, y]
}

pub fn scale(sx: f64, sy: f64) -> Matrix {
    [sx, 0.0, 0.0, sy, 0.0, 0.0]
}

fn matrix6(v: &Option<Vec<f64>>) -> Option<Matrix> {
    let t = v.as_ref()?;
    (t.len() == 6).then(|| [t[0], t[1], t[2], t[3], t[4], t[5]])
}

/// Where a shape's coordinates land in the graphic's parent space (the page
/// canvas, or the template box): the graphic's box offset, its own transform
/// (taken about the box centre), then the shape's transform.
///
/// The graphic-level transform (rotated shapes, 30 of them in the fixtures) is
/// interpreted as a rotation about the box centre; its stored origin (`$549`)
/// is not understood yet (docs/format.md §7). Check against the device.
pub fn shape_matrix(g: &folio_nmdl::Graphic, s: &folio_nmdl::Shape) -> Matrix {
    let mut m = translate(g.left, g.top);
    if let Some(gt) = matrix6(&g.transform) {
        let (cx, cy) = (g.width / 2.0, g.height / 2.0);
        m = mul(m, mul(translate(cx, cy), mul(gt, translate(-cx, -cy))));
    }
    match matrix6(&s.transform) {
        Some(st) => mul(m, st),
        None => m,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_grey() {
        assert_eq!(gray_for_density(0, 1.0), 0);
        assert_eq!(gray_for_density(0, 0.0), 255);
        assert_eq!(gray_for_density(0, 0.5), 128); // 255 - int(127.5)
        assert!((adjusted_density(0.5) - (1.0 - 0.5f64.powf(3.5))).abs() < 1e-12);
    }

    #[test]
    fn output_sizes() {
        assert_eq!(output_size((15624, 20832), 2480), (1860, 2480));
        assert_eq!(output_size((13726, 7350), 2480), (2480, 1328));
    }
}
