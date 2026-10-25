//! Raster backend: a page to an 8-bit grey image.
//!
//! Pen-like strokes are stroked as outlines with round caps and joins
//! (`tiny-skia`) and composited one stroke at a time. Pressure-shaded strokes
//! get kfxlib's pencil texture (docs/format.md §4.5): a density map in a bitmap of
//! `scale` canvas units per pixel, dithered with a PRNG seeded per stroke, then
//! spread onto the page with an exact area filter. Highlighters are drawn into
//! one layer that is composited at 20 % where kfxlib places its opacity group.

use folio_nmdl::{Graphic, Page, PathOp, ShapeGeometry, Stroke, Template};
use tiny_skia::{FillRule, LineCap, LineJoin, Mask, Path, PathBuilder, Rect, Stroke as SkStroke, Transform};

use crate::geom::{
    Matrix, StrokePath, adjusted_density, argb_gray, is_highlighter, mul, output_size, rgb_gray, scale, shape_matrix,
    stroke_matrix, stroke_paths,
};
use crate::rng::Xoshiro256pp;

/// kfxlib's `PNG_EDGE_FEATHERING`.
const FEATHER: f64 = 0.75;
const HIGHLIGHTER_OPACITY: f32 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PencilMode {
    /// kfxlib's stochastic texture, with a density bitmap of `scale` canvas units
    /// per pixel (kfxlib used 8; finer is sharper and slower).
    Texture { scale: u32 },
    /// Solid grey paths, as in vector export ("vector gray").
    Gray,
}

#[derive(Clone, Copy, Debug)]
pub struct RasterOptions {
    /// Output size of the page's long edge, in pixels.
    pub long_edge: u32,
    pub pencil: PencilMode,
    /// Draw `$272` vector graphics (kfxlib skipped them).
    pub shapes: bool,
}

impl Default for RasterOptions {
    fn default() -> Self {
        RasterOptions {
            long_edge: 2480,
            pencil: PencilMode::Texture { scale: 4 },
            shapes: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrayImage {
    pub width: u32,
    pub height: u32,
    /// Row-major, 0 black … 255 white.
    pub data: Vec<u8>,
}

impl GrayImage {
    pub fn white(width: u32, height: u32) -> GrayImage {
        GrayImage {
            width,
            height,
            data: vec![255; width as usize * height as usize],
        }
    }

    /// 8-bit grey PNG with fast compression (reader images, PLAN §3 decision 8).
    pub fn to_png(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.data.len() / 8);
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::Grayscale);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(png::Compression::Fast);
            let mut w = enc.write_header().expect("PNG header to a Vec");
            w.write_image_data(&self.data).expect("PNG data to a Vec");
        }
        out
    }
}

impl GrayImage {
    /// Grey + alpha PNG of the ink only (black, alpha = 255 − grey), for
    /// drawing over another page such as a PDF.
    pub fn to_ink_png(&self) -> Vec<u8> {
        let mut px = Vec::with_capacity(self.data.len() * 2);
        for &v in &self.data {
            px.extend_from_slice(&[0, 255 - v]);
        }
        let mut out = Vec::with_capacity(self.data.len() / 8);
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::GrayscaleAlpha);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(png::Compression::Fast);
            let mut w = enc.write_header().expect("PNG header to a Vec");
            w.write_image_data(&px).expect("PNG data to a Vec");
        }
        out
    }
}

/// A coverage mask placed at `(x, y)` in the output.
struct Placed {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    data: Vec<u8>,
}

impl Placed {
    fn from_mask(mask: Mask, x: u32, y: u32) -> Placed {
        Placed {
            x,
            y,
            w: mask.width(),
            h: mask.height(),
            data: mask.data().to_vec(),
        }
    }
}

const TILE: usize = 64;

/// A sparse grid of `f32`, allocated in 64×64 tiles as cells are touched.
/// Pencil strokes have large bounds but cover little of them.
struct Tiles {
    w: usize,
    h: usize,
    across: usize,
    tiles: Vec<Option<Box<[f32]>>>,
}

impl Tiles {
    fn new(w: usize, h: usize) -> Tiles {
        let across = w.div_ceil(TILE);
        Tiles {
            w,
            h,
            across,
            tiles: (0..across * h.div_ceil(TILE)).map(|_| None).collect(),
        }
    }

    fn cell(&mut self, x: usize, y: usize) -> &mut f32 {
        let tile = self.tiles[(y / TILE) * self.across + x / TILE].get_or_insert_with(|| vec![0.0; TILE * TILE].into());
        &mut tile[(y % TILE) * TILE + x % TILE]
    }

    /// Calls `f(x, y, value)` for every cell of every allocated tile, tile by tile.
    fn for_each(&self, mut f: impl FnMut(usize, usize, f32)) {
        for (i, tile) in self.tiles.iter().enumerate() {
            let Some(tile) = tile else { continue };
            let (tx, ty) = ((i % self.across) * TILE, (i / self.across) * TILE);
            for cy in 0..TILE.min(self.h - ty) {
                for cx in 0..TILE.min(self.w - tx) {
                    f(tx + cx, ty + cy, tile[cy * TILE + cx]);
                }
            }
        }
    }
}

/// Coverage of one stroke: a dense mask (pen strokes) or sparse tiles (pencil).
enum Coverage {
    Dense(Placed),
    Sparse { x: u32, y: u32, tiles: Tiles },
}

fn composite_coverage(img: &mut GrayImage, c: &Coverage, gray: u8, alpha: f32) {
    match c {
        Coverage::Dense(p) => composite(img, p, gray, alpha),
        Coverage::Sparse { x, y, tiles } => {
            let g = f32::from(gray);
            let width = img.width as usize;
            tiles.for_each(|cx, cy, v| {
                if v > 0.0 {
                    let d = &mut img.data[(*y as usize + cy) * width + *x as usize + cx];
                    let a = v.min(1.0) * alpha;
                    *d = (f32::from(*d) + (g - f32::from(*d)) * a + 0.5) as u8;
                }
            });
        }
    }
}

fn union_coverage(layer: &mut [u8], width: u32, c: &Coverage) {
    match c {
        Coverage::Dense(p) => union(layer, width, p),
        Coverage::Sparse { x, y, tiles } => tiles.for_each(|cx, cy, v| {
            if v > 0.0 {
                let d = &mut layer[(*y as usize + cy) * width as usize + *x as usize + cx];
                let (a, b) = (u32::from(*d), (v.min(1.0) * 255.0 + 0.5) as u32);
                *d = (a + b - (a * b + 127) / 255) as u8;
            }
        }),
    }
}

/// Source-over of `gray` at `alpha × coverage`.
fn composite(img: &mut GrayImage, m: &Placed, gray: u8, alpha: f32) {
    let g = f32::from(gray);
    for row in 0..m.h {
        let src = &m.data[(row * m.w) as usize..((row + 1) * m.w) as usize];
        let start = ((m.y + row) * img.width + m.x) as usize;
        let dst = &mut img.data[start..start + m.w as usize];
        for (d, &c) in dst.iter_mut().zip(src) {
            if c != 0 {
                let a = f32::from(c) * (alpha / 255.0);
                *d = (f32::from(*d) + (g - f32::from(*d)) * a + 0.5) as u8;
            }
        }
    }
}

/// Union of coverage into a page-sized layer.
fn union(layer: &mut [u8], width: u32, m: &Placed) {
    for row in 0..m.h {
        let src = &m.data[(row * m.w) as usize..((row + 1) * m.w) as usize];
        let start = ((m.y + row) * width + m.x) as usize;
        for (d, &c) in layer[start..start + m.w as usize].iter_mut().zip(src) {
            let (a, b) = (u32::from(*d), u32::from(c));
            *d = (a + b - (a * b + 127) / 255) as u8;
        }
    }
}

fn sk(m: Matrix) -> Transform {
    Transform::from_row(
        m[0] as f32,
        m[1] as f32,
        m[2] as f32,
        m[3] as f32,
        m[4] as f32,
        m[5] as f32,
    )
}

/// Linear scale of a matrix (for stroke tolerances and widths).
fn mean_scale(m: Matrix) -> f64 {
    (m[0] * m[3] - m[1] * m[2]).abs().sqrt()
}

/// Rasterises a filled outline (already in output coordinates) into a mask clipped to the image.
fn fill(path: &Path, width: u32, height: u32) -> Option<Placed> {
    let b = path.bounds();
    let x0 = (b.left().floor().max(0.0)) as u32;
    let y0 = (b.top().floor().max(0.0)) as u32;
    let x1 = (b.right().ceil().min(width as f32)).max(0.0) as u32;
    let y1 = (b.bottom().ceil().min(height as f32)).max(0.0) as u32;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let mut mask = Mask::new(x1 - x0, y1 - y0)?;
    mask.fill_path(
        path,
        FillRule::Winding,
        true,
        Transform::from_translate(-(x0 as f32), -(y0 as f32)),
    );
    Some(Placed::from_mask(mask, x0, y0))
}

fn polyline(points: &[(i64, i64)]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let (x, y) = points[0];
    pb.move_to(x as f32, y as f32);
    for &(x, y) in &points[1..] {
        pb.line_to(x as f32, y as f32);
    }
    pb.finish()
}

/// Outlines of a stroke's paths, grouped into runs of the same grey, in output coordinates.
fn pen_outlines(paths: &[StrokePath], ctm: Matrix) -> Vec<(u8, Path)> {
    let res = mean_scale(ctm).max(1e-6) as f32;
    let ts = sk(ctm);
    let mut groups: Vec<(u8, PathBuilder)> = Vec::new();
    for p in paths {
        let Some(path) = polyline(&p.points) else { continue };
        let style = SkStroke {
            width: p.width as f32,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..SkStroke::default()
        };
        let Some(outline) = path.stroke(&style, res).and_then(|o| o.transform(ts)) else {
            continue;
        };
        match groups.last_mut() {
            Some((g, pb)) if *g == p.gray => pb.push_path(&outline),
            _ => {
                let mut pb = PathBuilder::new();
                pb.push_path(&outline);
                groups.push((p.gray, pb));
            }
        }
    }
    groups
        .into_iter()
        .filter_map(|(g, pb)| Some((g, pb.finish()?)))
        .collect()
}

/// kfxlib's pencil texture for one stroke, as coverage in output pixels.
fn pencil_coverage(stroke: &Stroke, scale_units: u32, ctm: Matrix, width: u32, height: u32) -> Option<Coverage> {
    let s = i64::from(scale_units.max(1));
    let b = stroke.bounds;
    let (bw, bh) = (b[2] - b[0], b[3] - b[1]);
    let (w, h) = (bw.div_euclid(s), bh.div_euclid(s));
    if w <= 0 || h <= 0 {
        return None;
    }

    // Points in bitmap pixels, with kfxlib's midpoint subdivision between them.
    let mut pts: Vec<(i64, i64, f64, f64)> = Vec::with_capacity(stroke.points.len() * 2);
    let mut last: Option<(i64, i64, f64, f64)> = None;
    for p in &stroke.points {
        let cur = (
            (p.x - b[0]).div_euclid(s),
            (p.y - b[1]).div_euclid(s),
            p.t as f64 / (2 * s) as f64,
            p.d,
        );
        if let Some(prev) = last {
            subdivide(&mut pts, prev, cur);
        }
        pts.push(cur);
        last = Some(cur);
    }

    let mut rng = Xoshiro256pp::seed_from_u64(stroke.seed.unwrap_or(0) as u64);
    let mut density = Tiles::new(w as usize, h as usize);
    for &(x, y, r, d) in &pts {
        let r = if r <= 0.0 { 0.5 } else { r };
        let dd = adjusted_density(d) as f32;
        let ir = (r * 1.5).ceil() as i64;
        let solid = (FEATHER * r) * (FEATHER * r);
        // ink beyond (2 - FEATHER)·r is impossible: skip it without drawing a number
        let reach = (2.0 - FEATHER) * r;
        let reach2 = reach * reach;
        for xx in (x - ir).max(0)..=(x + ir).min(w - 1) {
            for yy in (y - ir).max(0)..=(y + ir).min(h - 1) {
                let (dx, dy) = ((xx - x) as f64, (yy - y) as f64);
                let dist2 = dx * dx + dy * dy;
                let cell = density.cell(xx as usize, yy as usize);
                if dist2 <= solid {
                    *cell = cell.max(dd);
                } else if dist2 <= reach2 {
                    let rel = dist2.sqrt() / r;
                    if rel <= rng.next_f64() * (2.0 - FEATHER) {
                        *cell = cell.max(dd * ((2.0 - FEATHER) - rel) as f32);
                    }
                }
            }
        }
    }

    // The bitmap is stretched over the stroke bounds.
    let (px_w, px_h) = (bw as f64 / w as f64, bh as f64 / h as f64);
    let corners = [(b[0], b[1]), (b[2], b[1]), (b[0], b[3]), (b[2], b[3])].map(|(x, y)| apply(ctm, x as f64, y as f64));
    let bx0 = corners.iter().map(|c| c.0).fold(f64::MAX, f64::min).floor().max(0.0) as u32;
    let by0 = corners.iter().map(|c| c.1).fold(f64::MAX, f64::min).floor().max(0.0) as u32;
    let bx1 = (corners.iter().map(|c| c.0).fold(f64::MIN, f64::max).ceil().max(0.0) as u32).min(width);
    let by1 = (corners.iter().map(|c| c.1).fold(f64::MIN, f64::max).ceil().max(0.0) as u32).min(height);
    if bx1 <= bx0 || by1 <= by0 {
        return None;
    }
    let (cw, ch) = (bx1 - bx0, by1 - by0);
    let mut cov = Tiles::new(cw as usize, ch as usize);
    // pixel extent in output units (rotation is approximated by its scale)
    let (ex, ey) = (px_w * (ctm[0].hypot(ctm[1])), px_h * (ctm[2].hypot(ctm[3])));
    density.for_each(|i, j, dd| {
        if dd <= 0.0 || rng.next_f64() as f32 >= dd {
            return;
        }
        let (cx, cy) = apply(
            ctm,
            b[0] as f64 + (i as f64 + 0.5) * px_w,
            b[1] as f64 + (j as f64 + 0.5) * px_h,
        );
        splat(&mut cov, cx - ex / 2.0 - bx0 as f64, cy - ey / 2.0 - by0 as f64, ex, ey);
    });
    Some(Coverage::Sparse {
        x: bx0,
        y: by0,
        tiles: cov,
    })
}

/// kfxlib's `add_points_if_needed`: recursive midpoints until points are within max(r1, r2, 1.5).
fn subdivide(pts: &mut Vec<(i64, i64, f64, f64)>, a: (i64, i64, f64, f64), b: (i64, i64, f64, f64)) {
    let dist = (((a.0 - b.0) as f64).powi(2) + ((a.1 - b.1) as f64).powi(2)).sqrt();
    if dist > a.2.max(b.2).max(1.5) {
        let m = (
            (a.0 + b.0).div_euclid(2),
            (a.1 + b.1).div_euclid(2),
            (a.2 + b.2) / 2.0,
            (a.3 + b.3) / 2.0,
        );
        subdivide(pts, a, m);
        subdivide(pts, m, b);
        pts.push(m);
    }
}

fn apply(m: Matrix, x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

/// Adds the area of rectangle `(x, y, w, h)` that falls in each pixel.
fn splat(cov: &mut Tiles, x: f64, y: f64, w: f64, h: f64) {
    let (x1, y1) = (x + w, y + h);
    let (px0, px1) = (x.floor().max(0.0) as usize, (x1.ceil().max(0.0) as usize).min(cov.w));
    let (py0, py1) = (y.floor().max(0.0) as usize, (y1.ceil().max(0.0) as usize).min(cov.h));
    for py in py0..py1 {
        let oy = (y1.min((py + 1) as f64) - y.max(py as f64)).max(0.0);
        if oy <= 0.0 {
            continue;
        }
        for px in px0..px1 {
            let ox = (x1.min((px + 1) as f64) - x.max(px as f64)).max(0.0);
            *cov.cell(px, py) += (ox * oy) as f32;
        }
    }
}

/// Draws one stroke's coverage. Returns it rather than compositing, so
/// highlighters can be collected into their own layer.
fn stroke_coverage(stroke: &Stroke, page_ctm: Matrix, opts: &RasterOptions, w: u32, h: u32) -> Vec<(u8, Coverage)> {
    let ctm = match stroke_matrix(stroke) {
        Some(t) => mul(page_ctm, t),
        None => page_ctm,
    };
    if stroke.variable_density
        && let PencilMode::Texture { scale } = opts.pencil
    {
        let gray = rgb_gray(stroke.color.unwrap_or(0));
        return pencil_coverage(stroke, scale, ctm, w, h)
            .map(|p| (gray, p))
            .into_iter()
            .collect();
    }
    pen_outlines(&stroke_paths(stroke), ctm)
        .into_iter()
        .filter_map(|(g, path)| Some((g, Coverage::Dense(fill(&path, w, h)?))))
        .collect()
}

fn shape_path(geometry: &ShapeGeometry) -> Option<Path> {
    let mut pb = PathBuilder::new();
    match geometry {
        ShapeGeometry::Path(ops) => {
            for op in ops {
                match *op {
                    PathOp::Move(x, y) => pb.move_to(x as f32, y as f32),
                    PathOp::Line(x, y) => pb.line_to(x as f32, y as f32),
                    PathOp::Quad(a, b, x, y) => pb.quad_to(a as f32, b as f32, x as f32, y as f32),
                    PathOp::Cubic(a, b, c, d, x, y) => {
                        pb.cubic_to(a as f32, b as f32, c as f32, d as f32, x as f32, y as f32)
                    }
                    PathOp::Close => pb.close(),
                }
            }
            pb.finish()
        }
        ShapeGeometry::Ellipse { cx, cy, rx, ry } => PathBuilder::from_oval(Rect::from_ltrb(
            (cx - rx) as f32,
            (cy - ry) as f32,
            (cx + rx) as f32,
            (cy + ry) as f32,
        )?),
        ShapeGeometry::Rectangle { x, y, width, height } => Some(PathBuilder::from_rect(Rect::from_xywh(
            *x as f32,
            *y as f32,
            *width as f32,
            *height as f32,
        )?)),
        ShapeGeometry::Polyline(v) | ShapeGeometry::Polygon(v) => {
            let mut it = v.as_chunks::<2>().0.iter();
            let [x0, y0] = *it.next()?;
            pb.move_to(x0 as f32, y0 as f32);
            for &[x, y] in it {
                pb.line_to(x as f32, y as f32);
            }
            if matches!(geometry, ShapeGeometry::Polygon(_)) {
                pb.close();
            }
            pb.finish()
        }
        ShapeGeometry::Unknown => None,
    }
}

/// Draws vector graphics (shapes, template rules and dots) with `parent` mapping
/// their parent space to output pixels.
fn draw_graphics(img: &mut GrayImage, graphics: &[Graphic], parent: Matrix) {
    for g in graphics {
        for s in &g.shapes {
            let Some(path) = shape_path(&s.geometry) else { continue };
            let m = mul(parent, shape_matrix(g, s));
            if let Some(fill_argb) = s.fill
                && let Some(p) = path.clone().transform(sk(m))
                && let Some(placed) = fill(&p, img.width, img.height)
            {
                let (gray, a) = argb_gray(fill_argb);
                composite(img, &placed, gray, (a * s.fill_opacity.unwrap_or(1.0)) as f32);
            }
            if let Some(stroke_argb) = s.stroke {
                let style = SkStroke {
                    width: s.stroke_width.unwrap_or(1.0) as f32,
                    ..SkStroke::default()
                };
                if let Some(outline) = path
                    .stroke(&style, mean_scale(m).max(1e-6) as f32)
                    .and_then(|o| o.transform(sk(m)))
                    && let Some(placed) = fill(&outline, img.width, img.height)
                {
                    let (gray, a) = argb_gray(stroke_argb);
                    composite(img, &placed, gray, a as f32);
                }
            }
        }
    }
}

/// A template drawn at `width`×`height` pixels, to put under a page's strokes.
pub fn render_template(template: &Template, width: u32, height: u32) -> GrayImage {
    let mut img = GrayImage::white(width, height);
    let m = scale(f64::from(width) / template.width, f64::from(height) / template.height);
    draw_graphics(&mut img, &template.graphics, m);
    img
}

/// Renders a page. `template` is the page's template already drawn at the
/// output size (see [`render_template`]); it is copied as the background.
pub fn render_page(page: &Page, template: Option<&GrayImage>, opts: &RasterOptions) -> GrayImage {
    let (w, h) = output_size(page.canvas, opts.long_edge);
    let mut img = match template {
        Some(t) if t.width == w && t.height == h => t.clone(),
        _ => GrayImage::white(w, h),
    };
    let ctm = scale(
        f64::from(w) / page.canvas.0.max(1) as f64,
        f64::from(h) / page.canvas.1.max(1) as f64,
    );

    // kfxlib puts every highlighter into one opacity group where the first one
    // occurs; strokes after it are drawn over the group.
    let first_hl = page.strokes.iter().position(is_highlighter);
    let split = first_hl.unwrap_or(page.strokes.len());
    for s in &page.strokes[..split] {
        for (gray, cov) in stroke_coverage(s, ctm, opts, w, h) {
            composite_coverage(&mut img, &cov, gray, 1.0);
        }
    }
    if first_hl.is_some() {
        let mut layer = vec![0u8; (w * h) as usize];
        let mut gray = 0;
        for s in page.strokes.iter().filter(|s| is_highlighter(s)) {
            for (g, cov) in stroke_coverage(s, ctm, opts, w, h) {
                gray = g;
                union_coverage(&mut layer, w, &cov);
            }
        }
        let whole = Placed {
            x: 0,
            y: 0,
            w,
            h,
            data: layer,
        };
        composite(&mut img, &whole, gray, HIGHLIGHTER_OPACITY);
        for s in page.strokes[split..].iter().filter(|s| !is_highlighter(s)) {
            for (gray, cov) in stroke_coverage(s, ctm, opts, w, h) {
                composite_coverage(&mut img, &cov, gray, 1.0);
            }
        }
    }
    if opts.shapes {
        draw_graphics(&mut img, &page.graphics, ctm);
    }
    img
}
