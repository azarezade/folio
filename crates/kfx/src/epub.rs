//! EPUB metadata: the OPF's title, creators, language and cover image, and
//! DRM from `META-INF/encryption.xml` / `rights.xml`.

use std::path::Path;

use crate::BookInfo;
use crate::xml::{Tag, tags};
use crate::zip::Zip;

/// Font obfuscation isn't DRM; anything else in encryption.xml is.
const FONT_OBFUSCATION: [&str; 2] = ["http://www.idpf.org/2008/embedding", "http://ns.adobe.com/pdf/enc#RC"];

fn bad(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())
}

fn read_text(zip: &mut Zip, name: &str) -> std::io::Result<Option<String>> {
    Ok(zip
        .read(name, 8 << 20)?
        .map(|b| String::from_utf8_lossy(&b).into_owned()))
}

/// Resolves `href` relative to the directory of `base` (both ZIP paths).
pub(crate) fn resolve(base: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or(href);
    let href = percent_decode(href);
    let mut parts: Vec<&str> = base.rsplit_once('/').map_or(vec![], |(d, _)| d.split('/').collect());
    for seg in href.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

struct Opf {
    path: String,
    tags: Vec<Tag>,
}

fn opf(zip: &mut Zip) -> std::io::Result<Opf> {
    let container = read_text(zip, "META-INF/container.xml")?.ok_or_else(|| bad("no META-INF/container.xml"))?;
    let path = tags(&container)
        .into_iter()
        .find(|t| t.name == "rootfile" && !t.close)
        .and_then(|t| t.attr("full-path").map(str::to_string))
        .ok_or_else(|| bad("no rootfile in container.xml"))?;
    let doc = read_text(zip, &path)?.ok_or_else(|| bad("the package document is missing"))?;
    Ok(Opf { path, tags: tags(&doc) })
}

fn drm(zip: &mut Zip) -> std::io::Result<Option<String>> {
    if zip.contains("META-INF/rights.xml") {
        return Ok(Some(
            "This EPUB is protected by Adobe DRM, so it can't be opened here.".into(),
        ));
    }
    if zip.contains("META-INF/sinf.xml") {
        return Ok(Some(
            "This EPUB is protected by Apple FairPlay DRM, so it can't be opened here.".into(),
        ));
    }
    if let Some(enc) = read_text(zip, "META-INF/encryption.xml")? {
        let protected = tags(&enc)
            .iter()
            .filter(|t| t.name == "EncryptionMethod" && !t.close)
            .filter_map(|t| t.attr("Algorithm"))
            .any(|a| !FONT_OBFUSCATION.contains(&a));
        if protected {
            return Ok(Some(
                "This EPUB is protected by DRM, so it can't be opened here.".into(),
            ));
        }
    }
    Ok(None)
}

fn cover_href(opf: &Opf) -> Option<String> {
    let items = || opf.tags.iter().filter(|t| t.name == "item" && !t.close);
    let is_image = |t: &&Tag| t.attr("media-type").is_some_and(|m| m.starts_with("image/"));
    // EPUB 3: properties="cover-image"; EPUB 2: <meta name="cover" content="id">.
    let found = items()
        .find(|t| {
            t.attr("properties")
                .is_some_and(|p| p.split_whitespace().any(|p| p == "cover-image"))
        })
        .or_else(|| {
            let id = opf
                .tags
                .iter()
                .find(|t| t.name == "meta" && t.attr("name") == Some("cover"))?
                .attr("content")?;
            items().filter(is_image).find(|t| t.attr("id") == Some(id))
        })
        .or_else(|| {
            items()
                .filter(is_image)
                .find(|t| t.attr("id").is_some_and(|i| i.to_ascii_lowercase().contains("cover")))
        })?;
    Some(resolve(&opf.path, found.attr("href")?))
}

pub fn inspect(path: &Path) -> std::io::Result<BookInfo> {
    let mut zip = Zip::open(path)?;
    let blocked = drm(&mut zip)?;
    let opf = opf(&mut zip)?;
    let texts = |name: &str| -> Vec<String> {
        opf.tags
            .iter()
            .filter(|t| t.name == name && !t.close && !t.text.is_empty())
            .map(|t| t.text.clone())
            .collect()
    };
    let mut authors = texts("creator");
    authors.dedup();
    let has_cover = cover_href(&opf).is_some_and(|h| zip.contains(&h));
    Ok(BookInfo {
        title: texts("title").into_iter().next(),
        authors,
        language: texts("language").into_iter().next(),
        asin: None,
        dictionary: false,
        blocked,
        has_cover,
    })
}

pub fn cover(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    let mut zip = Zip::open(path)?;
    let opf = opf(&mut zip)?;
    match cover_href(&opf) {
        Some(h) => zip.read(&h, 16 << 20),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_hrefs() {
        assert_eq!(
            resolve("OEBPS/content.opf", "images/cover%20a.jpg"),
            "OEBPS/images/cover a.jpg"
        );
        assert_eq!(resolve("content.opf", "x.jpg"), "x.jpg");
        assert_eq!(resolve("a/b/c.opf", "../i/x.png#f"), "a/i/x.png");
    }
}
