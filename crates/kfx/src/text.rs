//! Plain text of a book, for full-text search: EPUB spine documents, MOBI /
//! AZW3 text records (PalmDoc-compressed or uncompressed), PDF pages and
//! text files. Each chunk carries a location the reader can jump to:
//! `section:<spine index>` (EPUB), `fraction:<0..1>` (MOBI/AZW3),
//! `page:<index>` (PDF), or empty (text).

use std::path::Path;

use crate::xml::{tags, unescape};
use crate::zip::Zip;
use crate::{Kind, detect};

pub struct Chunk {
    pub loc: String,
    /// A short label for the result list: "Chapter 3", "p. 12", "42%".
    pub label: String,
    pub text: String,
}

fn bad(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())
}

/// Text of an HTML/XHTML document: tags removed, entities decoded, scripts and
/// styles dropped, whitespace collapsed. Block tags become spaces.
pub fn html_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        out.push_str(&unescape(&rest[..lt]));
        rest = &rest[lt..];
        let lower = rest.get(..8).unwrap_or(rest).to_ascii_lowercase();
        let skip_to = if lower.starts_with("<script") {
            Some("</script")
        } else if lower.starts_with("<style") {
            Some("</style")
        } else if lower.starts_with("<!--") {
            Some("-->")
        } else {
            None
        };
        if let Some(end) = skip_to {
            let lower_rest = rest.to_ascii_lowercase();
            rest = lower_rest.find(end).map_or("", |i| &rest[i + end.len()..]);
            rest = rest.find('>').map_or(rest, |i| &rest[i + 1..]);
            continue;
        }
        let Some(gt) = rest.find('>') else { break };
        out.push(' ');
        rest = &rest[gt + 1..];
    }
    out.push_str(&unescape(rest));
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn extract(path: &Path) -> std::io::Result<Vec<Chunk>> {
    match detect(path)? {
        Kind::Epub => epub(path),
        Kind::Mobi => mobi(path),
        Kind::Pdf => pdf(path),
        Kind::Text => {
            let b = std::fs::read(path)?;
            let text = String::from_utf8_lossy(&b).trim_start_matches('\u{feff}').to_string();
            Ok(vec![Chunk {
                loc: String::new(),
                label: String::new(),
                text,
            }])
        }
        _ => Ok(Vec::new()),
    }
}

fn epub(path: &Path) -> std::io::Result<Vec<Chunk>> {
    let mut zip = Zip::open(path)?;
    let container = zip
        .read("META-INF/container.xml", 1 << 20)?
        .ok_or_else(|| bad("no container.xml"))?;
    let opf_path = tags(&String::from_utf8_lossy(&container))
        .into_iter()
        .find(|t| t.name == "rootfile" && !t.close)
        .and_then(|t| t.attr("full-path").map(str::to_string))
        .ok_or_else(|| bad("no rootfile"))?;
    let opf = zip
        .read(&opf_path, 8 << 20)?
        .ok_or_else(|| bad("no package document"))?;
    let opf_tags = tags(&String::from_utf8_lossy(&opf));
    let manifest: std::collections::HashMap<&str, &str> = opf_tags
        .iter()
        .filter(|t| t.name == "item" && !t.close)
        .filter_map(|t| Some((t.attr("id")?, t.attr("href")?)))
        .collect();
    let mut out = Vec::new();
    let spine = opf_tags.iter().filter(|t| t.name == "itemref" && !t.close);
    for (index, item) in spine.enumerate() {
        let Some(href) = item.attr("idref").and_then(|id| manifest.get(id)) else {
            continue;
        };
        let full = crate::epub::resolve(&opf_path, href);
        let Some(doc) = zip.read(&full, 32 << 20)? else {
            continue;
        };
        let doc = String::from_utf8_lossy(&doc);
        let label = tags(&doc)
            .into_iter()
            .find(|t| !t.close && matches!(t.name.as_str(), "h1" | "h2" | "h3" | "title") && !t.text.is_empty())
            .map(|t| t.text)
            .unwrap_or_default();
        let text = html_text(&doc);
        if !text.is_empty() {
            out.push(Chunk {
                loc: format!("section:{index}"),
                label,
                text,
            });
        }
    }
    Ok(out)
}

/// PalmDoc LZ77 (compression type 2).
pub fn palmdoc_decompress(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 2);
    let mut i = 0;
    while i < data.len() {
        let c = data[i];
        i += 1;
        match c {
            1..=8 => {
                let end = (i + c as usize).min(data.len());
                out.extend_from_slice(&data[i..end]);
                i = end;
            }
            0x80..=0xBF => {
                let Some(&next) = data.get(i) else { break };
                i += 1;
                let x = (((c as usize) << 8) | next as usize) & 0x3FFF;
                let (dist, len) = (x >> 3, (x & 7) + 3);
                if dist == 0 || dist > out.len() {
                    continue;
                }
                let start = out.len() - dist;
                for k in 0..len {
                    out.push(out[start + k]);
                }
            }
            0xC0..=0xFF => {
                out.push(b' ');
                out.push(c ^ 0x80);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Bytes of trailing entries at the end of a text record (MOBI "extra data flags").
fn trailing_size(data: &[u8], flags: u16) -> usize {
    let mut num = 0usize;
    let mut f = flags >> 1;
    while f != 0 {
        if f & 1 != 0 {
            // backward-encoded variable-width size
            let mut size = data.len().saturating_sub(num);
            let (mut bitpos, mut result) = (0, 0usize);
            while size > 0 {
                let v = data[size - 1];
                result |= ((v & 0x7F) as usize) << bitpos;
                bitpos += 7;
                size -= 1;
                if v & 0x80 != 0 || bitpos >= 28 {
                    break;
                }
            }
            num += result;
        }
        f >>= 1;
    }
    if flags & 1 != 0
        && let Some(&b) = data.len().checked_sub(num + 1).and_then(|i| data.get(i))
    {
        num += (b & 3) as usize + 1;
    }
    num.min(data.len())
}

fn mobi(path: &Path) -> std::io::Result<Vec<Chunk>> {
    let mut palm = crate::mobi::Palm::open(path)?;
    let r0 = palm.record(0, 1 << 20)?;
    let be16 = |o: usize| r0.get(o..o + 2).map_or(0, |b| u16::from_be_bytes([b[0], b[1]]));
    let be32 = |o: usize| {
        r0.get(o..o + 4)
            .map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let (compression, records, encryption) = (be16(0), be16(8) as usize, be16(12));
    if encryption != 0 || !matches!(compression, 1 | 2) {
        // DRM, or HUFF/CDIC compression (not indexed)
        return Ok(Vec::new());
    }
    let has_mobi = r0.get(16..20) == Some(b"MOBI");
    let utf8 = has_mobi && be32(28) == 65001;
    let flags = if has_mobi && be32(20) >= 0xE4 { be16(0xF2) } else { 0 };
    let mut raw = Vec::new();
    for i in 1..=records {
        let rec = palm.record(i, 1 << 20)?;
        let body = &rec[..rec.len() - trailing_size(&rec, flags)];
        if compression == 2 {
            raw.extend(palmdoc_decompress(body));
        } else {
            raw.extend_from_slice(body);
        }
    }
    let html: String = if utf8 {
        String::from_utf8_lossy(&raw).into_owned()
    } else {
        raw.iter().map(|&c| c as char).collect()
    };
    // ~32 KB slices of the markup, cut at tag boundaries, located by fraction
    let total = html.len().max(1);
    let mut out = Vec::new();
    let mut start = 0;
    while start < html.len() {
        let mut end = (start + 32 * 1024).min(html.len());
        while end < html.len() && !html.is_char_boundary(end) {
            end += 1;
        }
        if end < html.len()
            && let Some(gt) = html[end..].find('>')
        {
            end += gt + 1;
        }
        let text = html_text(&html[start..end]);
        if !text.is_empty() {
            let fraction = start as f64 / total as f64;
            out.push(Chunk {
                loc: format!("fraction:{fraction:.5}"),
                label: format!("{}%", (fraction * 100.0).round()),
                text,
            });
        }
        start = end;
    }
    Ok(out)
}

fn pdf(path: &Path) -> std::io::Result<Vec<Chunk>> {
    let doc = lopdf::Document::load(path).map_err(|e| bad(&e.to_string()))?;
    let mut out = Vec::new();
    let pages: Vec<u32> = doc.get_pages().keys().copied().collect();
    for (index, n) in pages.into_iter().enumerate() {
        let Ok(text) = doc.extract_text(&[n]) else { continue };
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !text.is_empty() {
            out.push(Chunk {
                loc: format!("page:{index}"),
                label: format!("p. {}", index + 1),
                text,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_html() {
        assert_eq!(
            html_text("<p>A&amp;B</p><script>x()</script><style>p{}</style><p>c<br/>d</p>"),
            "A&B c d"
        );
    }

    #[test]
    fn palmdoc() {
        // literal, then a back-reference (distance 3, length 3), then a space pair
        assert_eq!(palmdoc_decompress(b"abc\x80\x18\xe1"), b"abcabc a");
        assert_eq!(palmdoc_decompress(b"\x02xy"), b"xy");
    }

    #[test]
    fn trailing_entries() {
        // flag bit 1: a one-byte size entry 0x83 (3 bytes, including itself)
        assert_eq!(trailing_size(b"textab\x83", 2), 3);
        // flag bit 0: multibyte overlap, low two bits + 1
        assert_eq!(trailing_size(b"text\x01", 1), 2);
    }
}
