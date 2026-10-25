//! Format detection, DRM, metadata and covers on small hand-built books, plus
//! every book in a real library when `FOLIO_TEST_LIBRARY` names one.

use std::path::{Path, PathBuf};

use folio_kfx::{Kind, cover, inspect};

const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0, 0xFF, 0xD9];

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("folio-kfx-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// A stored (uncompressed) ZIP.
fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut cd = Vec::new();
    for (name, data) in entries {
        let off = out.len() as u32;
        let crc = 0u32; // not checked by the reader
        let mut local = Vec::new();
        local.extend(b"PK\x03\x04");
        local.extend(20u16.to_le_bytes());
        local.extend([0u8; 4]); // flags, method 0
        local.extend([0u8; 4]); // time, date
        local.extend(crc.to_le_bytes());
        local.extend((data.len() as u32).to_le_bytes());
        local.extend((data.len() as u32).to_le_bytes());
        local.extend((name.len() as u16).to_le_bytes());
        local.extend(0u16.to_le_bytes());
        local.extend(name.as_bytes());
        out.extend(&local);
        out.extend(*data);
        cd.extend(b"PK\x01\x02");
        cd.extend([20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        cd.extend(crc.to_le_bytes());
        cd.extend((data.len() as u32).to_le_bytes());
        cd.extend((data.len() as u32).to_le_bytes());
        cd.extend((name.len() as u16).to_le_bytes());
        cd.extend([0u8; 12]); // extra, comment, disk, internal, external attrs
        cd.extend(off.to_le_bytes());
        cd.extend(name.as_bytes());
    }
    let cd_off = out.len() as u32;
    out.extend(&cd);
    out.extend(b"PK\x05\x06");
    out.extend([0u8; 4]);
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend((cd.len() as u32).to_le_bytes());
    out.extend(cd_off.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

fn epub(extra: &[(&str, &[u8])]) -> Vec<u8> {
    let opf = br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>The Test &amp; Book</dc:title>
<dc:creator id="a1">Ada Lovelace</dc:creator><dc:creator>Charles Babbage</dc:creator><dc:language>en</dc:language></metadata>
<manifest><item id="c" href="images/cover.jpg" media-type="image/jpeg" properties="cover-image"/>
<item id="t" href="text.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="t"/></spine></package>"#;
    let mut entries: Vec<(&str, &[u8])> = vec![
        ("mimetype", b"application/epub+zip"),
        (
            "META-INF/container.xml",
            br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        ),
        ("OEBPS/content.opf", opf),
        ("OEBPS/images/cover.jpg", JPEG),
        ("OEBPS/text.xhtml", b"<html><body><p>Hi</p></body></html>"),
    ];
    entries.extend_from_slice(extra);
    zip(&entries)
}

fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}
fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

/// A MOBI with a title, an author, a cover and the given encryption.
fn mobi(encryption: u16, dictionary: bool) -> Vec<u8> {
    let mut exth = Vec::new();
    let recs: Vec<(u32, Vec<u8>)> = vec![
        (100, b"Jane Austen".to_vec()),
        (503, b"Pride and Prejudice".to_vec()),
        (201, be32(0).to_vec()),
        (113, b"B000TEST00".to_vec()),
    ];
    for (t, v) in &recs {
        exth.extend(be32(*t));
        exth.extend(be32(8 + v.len() as u32));
        exth.extend(v);
    }
    let exth_len = 12 + exth.len();
    let mut e = b"EXTH".to_vec();
    e.extend(be32(exth_len as u32));
    e.extend(be32(recs.len() as u32));
    e.extend(exth);
    let hlen = 232u32;
    let mut r0 = vec![0u8; 16 + hlen as usize];
    r0[0..2].copy_from_slice(&be16(2));
    r0[12..14].copy_from_slice(&be16(encryption));
    r0[16..20].copy_from_slice(b"MOBI");
    r0[20..24].copy_from_slice(&be32(hlen));
    r0[28..32].copy_from_slice(&be32(65001));
    r0[36..40].copy_from_slice(&be32(6));
    r0[40..44].copy_from_slice(&be32(if dictionary { 5 } else { u32::MAX }));
    r0[108..112].copy_from_slice(&be32(1));
    r0[128..132].copy_from_slice(&be32(0x40));
    r0.extend(e);
    let name_off = r0.len() as u32;
    r0.extend(b"Full Name");
    r0[84..88].copy_from_slice(&be32(name_off));
    r0[88..92].copy_from_slice(&be32(9));
    let records = [r0, JPEG.to_vec()];
    let mut out = vec![0u8; 78];
    out[..4].copy_from_slice(b"test");
    out[60..68].copy_from_slice(b"BOOKMOBI");
    out[76..78].copy_from_slice(&be16(records.len() as u16));
    let mut off = 78 + 8 * records.len() as u32 + 2;
    for r in &records {
        out.extend(be32(off));
        out.extend([0u8; 4]);
        off += r.len() as u32;
    }
    out.extend([0, 0]);
    for r in &records {
        out.extend(r);
    }
    out
}

#[test]
fn epub_metadata_and_cover() {
    let p = tmp("book.epub");
    std::fs::write(&p, epub(&[])).unwrap();
    let (kind, info) = inspect(&p);
    assert_eq!(kind, Kind::Epub);
    assert_eq!(info.title.as_deref(), Some("The Test & Book"));
    assert_eq!(info.authors, ["Ada Lovelace", "Charles Babbage"]);
    assert_eq!(info.language.as_deref(), Some("en"));
    assert!(info.blocked.is_none());
    assert!(info.has_cover);
    let c = cover(&p).unwrap();
    assert_eq!((c.mime, c.data.as_slice()), ("image/jpeg", JPEG));
}

#[test]
fn epub_drm() {
    let fonts = br#"<encryption><EncryptedData><EncryptionMethod Algorithm="http://www.idpf.org/2008/embedding"/></EncryptedData></encryption>"#;
    let p = tmp("fonts.epub");
    std::fs::write(&p, epub(&[("META-INF/encryption.xml", fonts)])).unwrap();
    assert!(inspect(&p).1.blocked.is_none(), "font obfuscation isn't DRM");

    let aes = br#"<encryption><EncryptedData><EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes128-cbc"/></EncryptedData></encryption>"#;
    std::fs::write(&p, epub(&[("META-INF/encryption.xml", aes)])).unwrap();
    assert!(inspect(&p).1.blocked.unwrap().contains("DRM"));

    std::fs::write(&p, epub(&[("META-INF/rights.xml", b"<rights/>")])).unwrap();
    assert!(inspect(&p).1.blocked.unwrap().contains("Adobe"));
}

#[test]
fn mobi_metadata_cover_and_drm() {
    let p = tmp("book.mobi");
    std::fs::write(&p, mobi(0, false)).unwrap();
    let (kind, info) = inspect(&p);
    assert_eq!(kind, Kind::Mobi);
    assert_eq!(info.title.as_deref(), Some("Pride and Prejudice"));
    assert_eq!(info.authors, ["Jane Austen"]);
    assert_eq!(info.asin.as_deref(), Some("B000TEST00"));
    assert!(info.blocked.is_none() && info.has_cover && !info.dictionary);
    assert_eq!(cover(&p).unwrap().data, JPEG);

    std::fs::write(&p, mobi(2, false)).unwrap();
    let info = inspect(&p).1;
    assert!(info.blocked.unwrap().contains("Amazon DRM"));
    assert_eq!(cover(&p).unwrap().data, JPEG, "covers aren't encrypted");

    std::fs::write(&p, mobi(2, true)).unwrap();
    let info = inspect(&p).1;
    assert!(info.dictionary && info.blocked.unwrap().contains("dictionary"));
}

#[test]
fn kfx_and_others() {
    let p = tmp("book.azw");
    std::fs::write(&p, b"\xeaDRMION\xee\x00\x00\x00").unwrap();
    let (kind, info) = inspect(&p);
    assert_eq!(kind, Kind::Kfx);
    assert!(info.blocked.unwrap().contains("Amazon DRM"));
    std::fs::write(&p, b"CONT\x02\x00").unwrap();
    assert!(inspect(&p).1.blocked.unwrap().contains("KFX"));

    let t = tmp("notes.txt");
    std::fs::write(&t, "hello").unwrap();
    assert_eq!(inspect(&t).0, Kind::Text);
    assert!(inspect(&t).1.blocked.is_none());

    let d = tmp("x.docx");
    std::fs::write(&d, zip(&[("word/document.xml", b"<w/>")])).unwrap();
    assert_eq!(inspect(&d).0, Kind::Docx);

    let damaged = tmp("bad.epub");
    std::fs::write(&damaged, b"PK\x03\x04garbage").unwrap();
    assert!(inspect(&damaged).1.blocked.unwrap().contains("damaged"));
}

/// Every book in a real library either opens or has a reason.
#[test]
fn user_library() {
    let Some(root) = std::env::var_os("FOLIO_TEST_LIBRARY").map(|l| Path::new(&l).join("books")) else {
        eprintln!("skipped: set FOLIO_TEST_LIBRARY to a Folio library folder");
        return;
    };
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if p.is_dir() {
                if !name.ends_with(".sdr") {
                    stack.push(p);
                }
                continue;
            }
            let (kind, info) = inspect(&p);
            eprintln!(
                "{name}: {kind:?} {:?} {:?} cover={} {:?}",
                info.title, info.authors, info.has_cover, info.blocked
            );
            if info.has_cover {
                assert!(cover(&p).is_some(), "{name}: cover listed but not readable");
            }
        }
    }
}

#[test]
fn text_for_search() {
    let p = tmp("text.epub");
    std::fs::write(&p, epub(&[])).unwrap();
    let chunks = folio_kfx::text::extract(&p).unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!((chunks[0].loc.as_str(), chunks[0].text.as_str()), ("section:0", "Hi"));

    // a one-page PDF with "Hello search" in Helvetica
    let content = "BT /F1 12 Tf 50 700 Td (Hello search) Tj ET";
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = pdf.len();
    pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for o in offs {
        pdf.extend(format!("{o:010} 00000 n \n").bytes());
    }
    pdf.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .bytes(),
    );
    let p = tmp("text.pdf");
    std::fs::write(&p, pdf).unwrap();
    let chunks = folio_kfx::text::extract(&p).unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].loc, "page:0");
    assert!(chunks[0].text.contains("Hello search"), "{:?}", chunks[0].text);
}
