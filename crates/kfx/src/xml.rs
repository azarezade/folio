//! Just enough XML for OPF and container files: a flat list of tags with
//! their attributes and the text that follows each one. Namespace prefixes
//! are dropped from names.

pub struct Tag {
    pub name: String,
    pub close: bool,
    pub attrs: Vec<(String, String)>,
    /// Text between this tag and the next.
    pub text: String,
}

impl Tag {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_string()
}

pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|&e| e < 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .or_else(|| ent.strip_prefix("#X"))
                .map(|h| u32::from_str_radix(h, 16))
                .or_else(|| ent.strip_prefix('#').map(str::parse::<u32>))
                .and_then(Result::ok)
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn tags(doc: &str) -> Vec<Tag> {
    let mut out = Vec::new();
    let mut rest = doc;
    while let Some(start) = rest.find('<') {
        rest = &rest[start..];
        if let Some(r) = rest.strip_prefix("<!--") {
            rest = r.find("-->").map_or("", |e| &r[e + 3..]);
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            rest = rest.find('>').map_or("", |e| &rest[e + 1..]);
            continue;
        }
        let Some(end) = rest.find('>') else { break };
        let inner = rest[1..end].trim_end_matches('/');
        let close = inner.starts_with('/');
        let inner = inner.trim_start_matches('/');
        let name_end = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
        let name = local(&inner[..name_end]);
        let mut attrs = Vec::new();
        let mut a = &inner[name_end..];
        while let Some(eq) = a.find('=') {
            let key = local(a[..eq].trim());
            let v = a[eq + 1..].trim_start();
            let Some(q) = v.chars().next().filter(|c| *c == '"' || *c == '\'') else {
                break;
            };
            let Some(vend) = v[1..].find(q) else { break };
            attrs.push((key, unescape(&v[1..1 + vend])));
            a = &v[vend + 2..];
        }
        rest = &rest[end + 1..];
        let text_end = rest.find('<').unwrap_or(rest.len());
        let text = unescape(rest[..text_end].trim());
        out.push(Tag {
            name,
            close,
            attrs,
            text,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_tags() {
        let t = tags(
            r#"<?xml version="1.0"?><!-- c --><dc:title id='t'>A &amp; B&#x2019;s</dc:title><meta name="cover" content="img"/>"#,
        );
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].name, "title");
        assert_eq!(t[0].text, "A & B\u{2019}s");
        assert_eq!(t[0].attr("id"), Some("t"));
        assert!(t[1].close);
        assert_eq!(t[2].attr("content"), Some("img"));
    }
}
