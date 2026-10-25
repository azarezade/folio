//! Streaming PDF writer for raster and vector notebook pages.
//!
//! Each page's objects are written to the output as soon as the page arrives;
//! only object offsets are kept, and the page tree, cross-reference table and
//! trailer are written at the end. Memory is about one encoded page.
//!
//! Raster pages are 8-bit grey images, Flate-compressed with the PNG "Up"
//! predictor (lossless, small for line art). Vector pages are Flate-compressed
//! content streams in canvas units; templates are Form XObjects written once
//! and reused, and highlighters are a transparency group at 20 % opacity.

use std::io::{self, Write};

use flate2::Compression;
use flate2::write::ZlibEncoder;
use folio_render::GrayImage;
use folio_render::vector::{HIGHLIGHTER_GSTATE, HIGHLIGHTER_XOBJECT, TEMPLATE_XOBJECT, VectorPage};

/// Physical size of the page's long edge in points. kfxlib's converter used
/// 2480 px at 300 dpi = 8.27 in, the Scribe's screen height.
pub const LONG_EDGE_PT: f64 = 2480.0 / 300.0 * 72.0;

const FLATE_LEVEL: u32 = 6;

/// A page ready to be written, encoded off the writer thread.
pub enum EncodedPage {
    Raster {
        width: u32,
        height: u32,
        /// zlib data with one PNG filter byte per row.
        data: Vec<u8>,
        size_pt: (f64, f64),
    },
    Vector {
        canvas: (i64, i64),
        /// zlib content stream, already mapped from canvas units to points.
        content: Vec<u8>,
        highlighter: Option<Vec<u8>>,
        /// The template form to draw under the page, from [`PdfWriter::add_template`].
        template: Option<TemplateRef>,
        size_pt: (f64, f64),
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemplateRef(u32);

/// Page size in points for a canvas: long edge [`LONG_EDGE_PT`].
pub fn page_size_pt(canvas: (i64, i64)) -> (f64, f64) {
    let (w, h) = (canvas.0.max(1) as f64, canvas.1.max(1) as f64);
    let k = LONG_EDGE_PT / w.max(h);
    (w * k, h * k)
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut z = ZlibEncoder::new(Vec::with_capacity(data.len() / 8), Compression::new(FLATE_LEVEL));
    z.write_all(data).expect("writing to a Vec");
    z.finish().expect("writing to a Vec")
}

/// Compresses a grey image for a raster page, using the PNG "Up" filter.
pub fn encode_raster(img: &GrayImage, size_pt: (f64, f64)) -> EncodedPage {
    let w = img.width as usize;
    let mut z = ZlibEncoder::new(Vec::with_capacity(img.data.len() / 8), Compression::new(FLATE_LEVEL));
    let mut row = vec![0u8; w + 1];
    row[0] = 2; // Up
    for (i, line) in img.data.chunks_exact(w).enumerate() {
        if i == 0 {
            row[1..].copy_from_slice(line);
        } else {
            let prev = &img.data[(i - 1) * w..i * w];
            for ((o, &c), &p) in row[1..].iter_mut().zip(line).zip(prev) {
                *o = c.wrapping_sub(p);
            }
        }
        z.write_all(&row).expect("writing to a Vec");
    }
    EncodedPage::Raster {
        width: img.width,
        height: img.height,
        data: z.finish().expect("writing to a Vec"),
        size_pt,
    }
}

/// Compresses a vector page's streams, wrapping the content in the transform
/// from canvas units (y down) to points (y up).
pub fn encode_vector(page: &VectorPage, canvas: (i64, i64), template: Option<TemplateRef>) -> EncodedPage {
    let size_pt = page_size_pt(canvas);
    let (cw, ch) = (canvas.0.max(1) as f64, canvas.1.max(1) as f64);
    let mut content = format!(
        "q {} 0 0 {} 0 {} cm\n",
        fmt_num(size_pt.0 / cw),
        fmt_num(-size_pt.1 / ch),
        fmt_num(size_pt.1)
    )
    .into_bytes();
    content.extend_from_slice(&page.content);
    content.extend_from_slice(b"Q\n");
    EncodedPage::Vector {
        canvas,
        content: deflate(&content),
        highlighter: page.highlighter.as_deref().map(deflate),
        template: if page.uses_template { template } else { None },
        size_pt,
    }
}

/// Counts bytes written, for the cross-reference table.
struct Counting<W> {
    inner: W,
    pos: u64,
}

impl<W: Write> Write for Counting<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.pos += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

const CATALOG: u32 = 1;
const PAGES: u32 = 2;

pub struct PdfWriter<W: Write> {
    out: Counting<W>,
    /// Byte offset of each object, by object number (index 0 unused).
    offsets: Vec<Option<u64>>,
    pages: Vec<u32>,
    highlighter_gstate: Option<u32>,
}

fn fmt_num(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl<W: Write> PdfWriter<W> {
    pub fn new(out: W) -> io::Result<Self> {
        let mut w = PdfWriter {
            out: Counting { inner: out, pos: 0 },
            offsets: vec![None, None, None],
            pages: Vec::new(),
            highlighter_gstate: None,
        };
        // binary marker comment so tools treat the file as binary
        w.out.write_all(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")?;
        Ok(w)
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    fn alloc(&mut self) -> u32 {
        self.offsets.push(None);
        (self.offsets.len() - 1) as u32
    }

    fn object(&mut self, id: u32, body: &[u8]) -> io::Result<()> {
        self.offsets[id as usize] = Some(self.out.pos);
        writeln!(self.out, "{id} 0 obj")?;
        self.out.write_all(body)?;
        self.out.write_all(b"\nendobj\n")
    }

    fn stream(&mut self, id: u32, dict: &str, data: &[u8]) -> io::Result<()> {
        self.offsets[id as usize] = Some(self.out.pos);
        write!(self.out, "{id} 0 obj\n<<{dict}/Length {}>>\nstream\n", data.len())?;
        self.out.write_all(data)?;
        self.out.write_all(b"\nendstream\nendobj\n")
    }

    /// Writes a template as a Form XObject in its own `width`×`height` space.
    pub fn add_template(&mut self, content: &[u8], width: f64, height: f64) -> io::Result<TemplateRef> {
        let id = self.alloc();
        let dict = format!(
            "/Type/XObject/Subtype/Form/BBox[0 0 {} {}]/Filter/FlateDecode",
            fmt_num(width),
            fmt_num(height)
        );
        self.stream(id, &dict, &deflate(content))?;
        Ok(TemplateRef(id))
    }

    pub fn add_page(&mut self, page: &EncodedPage) -> io::Result<()> {
        let page_id = self.alloc();
        let content_id = self.alloc();
        let (resources, content, size) = match page {
            EncodedPage::Raster {
                width,
                height,
                data,
                size_pt,
            } => {
                let image = self.alloc();
                let dict = format!(
                    "/Type/XObject/Subtype/Image/Width {width}/Height {height}/ColorSpace/DeviceGray\
                     /BitsPerComponent 8/Filter/FlateDecode\
                     /DecodeParms<</Predictor 15/Colors 1/BitsPerComponent 8/Columns {width}>>"
                );
                self.stream(image, &dict, data)?;
                let content = format!("q {} 0 0 {} 0 0 cm /Im0 Do Q\n", fmt_num(size_pt.0), fmt_num(size_pt.1));
                (
                    format!("/XObject<</Im0 {image} 0 R>>"),
                    deflate(content.as_bytes()),
                    *size_pt,
                )
            }
            EncodedPage::Vector {
                canvas,
                content,
                highlighter,
                template,
                size_pt,
            } => {
                let mut xobjects = String::new();
                let mut gstates = String::new();
                if let Some(TemplateRef(t)) = template {
                    xobjects.push_str(&format!("/{TEMPLATE_XOBJECT} {t} 0 R"));
                }
                if let Some(hl) = highlighter {
                    let form = self.alloc();
                    let dict = format!(
                        "/Type/XObject/Subtype/Form/BBox[0 0 {} {}]\
                         /Group<</S/Transparency/CS/DeviceGray>>/Filter/FlateDecode",
                        canvas.0, canvas.1
                    );
                    self.stream(form, &dict, hl)?;
                    xobjects.push_str(&format!("/{HIGHLIGHTER_XOBJECT} {form} 0 R"));
                    let gs = match self.highlighter_gstate {
                        Some(g) => g,
                        None => {
                            let g = self.alloc();
                            self.object(g, b"<</Type/ExtGState/CA 0.2/ca 0.2>>")?;
                            self.highlighter_gstate = Some(g);
                            g
                        }
                    };
                    gstates.push_str(&format!("/{HIGHLIGHTER_GSTATE} {gs} 0 R"));
                }
                let mut res = String::new();
                if !xobjects.is_empty() {
                    res.push_str(&format!("/XObject<<{xobjects}>>"));
                }
                if !gstates.is_empty() {
                    res.push_str(&format!("/ExtGState<<{gstates}>>"));
                }
                (res, content.clone(), *size_pt)
            }
        };
        self.stream(content_id, "/Filter/FlateDecode", &content)?;
        let body = format!(
            "<</Type/Page/Parent {PAGES} 0 R/MediaBox[0 0 {} {}]/Resources<<{resources}>>/Contents {content_id} 0 R>>",
            fmt_num(size.0),
            fmt_num(size.1)
        );
        self.object(page_id, body.as_bytes())?;
        self.pages.push(page_id);
        Ok(())
    }

    /// Writes the page tree, catalog, cross-reference table and trailer.
    pub fn finish(mut self, title: Option<&str>) -> io::Result<W> {
        let kids: Vec<String> = self.pages.iter().map(|p| format!("{p} 0 R")).collect();
        let pages = format!("<</Type/Pages/Kids[{}]/Count {}>>", kids.join(" "), self.pages.len());
        self.object(PAGES, pages.as_bytes())?;
        self.object(CATALOG, format!("<</Type/Catalog/Pages {PAGES} 0 R>>").as_bytes())?;
        let info = self.alloc();
        let mut info_body = String::from("<</Producer(Folio)");
        if let Some(t) = title {
            info_body.push_str(&format!("/Title{}", pdf_string(t)));
        }
        info_body.push_str(">>");
        self.object(info, info_body.as_bytes())?;

        let xref = self.out.pos;
        write!(self.out, "xref\n0 {}\n0000000000 65535 f \n", self.offsets.len())?;
        for off in &self.offsets[1..] {
            match off {
                Some(o) => writeln!(self.out, "{o:010} 00000 n ")?,
                None => self.out.write_all(b"0000000000 65535 f \n")?,
            }
        }
        write!(
            self.out,
            "trailer\n<</Size {}/Root {CATALOG} 0 R/Info {info} 0 R>>\nstartxref\n{xref}\n%%EOF\n",
            self.offsets.len()
        )?;
        self.out.flush()?;
        Ok(self.out.inner)
    }
}

/// A PDF string literal (UTF-16BE with BOM for non-ASCII text).
fn pdf_string(s: &str) -> String {
    if s.is_ascii() {
        let escaped: String = s
            .chars()
            .flat_map(|c| match c {
                '(' | ')' | '\\' => vec!['\\', c],
                c if (c as u32) < 32 => vec![' '],
                c => vec![c],
            })
            .collect();
        format!("({escaped})")
    } else {
        let mut hex = String::from("<FEFF");
        for u in s.encode_utf16() {
            hex.push_str(&format!("{u:04X}"));
        }
        hex.push('>');
        hex
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raster_pdf_structure() {
        let img = GrayImage::white(4, 3);
        let mut w = PdfWriter::new(Vec::new()).unwrap();
        w.add_page(&encode_raster(&img, (100.0, 75.0))).unwrap();
        let pdf = w.finish(Some("Notes (1)")).unwrap();
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.7"));
        assert!(text.contains("/Count 1"));
        assert!(text.contains("/Title(Notes \\(1\\))"));
        // every xref offset points at "N 0 obj"
        let xref_at: usize = text
            .rsplit("startxref\n")
            .next()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let table = &text[xref_at..];
        for (n, line) in table.lines().skip(3).take_while(|l| l.ends_with(" n ")).enumerate() {
            let off: usize = line[..10].parse().unwrap();
            assert!(text[off..].starts_with(&format!("{} 0 obj", n + 1)), "object {}", n + 1);
        }
    }
}
