//! Ion-text-like rendering of values, for `folio-cli inspect` and debugging.

use std::fmt::Write;

use crate::symtab::SymbolTable;
use crate::value::Value;

/// Limits that keep dumps of huge fragments readable.
#[derive(Clone, Copy, Debug)]
pub struct TextOptions {
    pub max_depth: usize,
    /// Elements shown per list or struct before `…`.
    pub max_items: usize,
    /// Bytes of blobs shown, as hex.
    pub max_blob: usize,
}

impl Default for TextOptions {
    fn default() -> Self {
        TextOptions {
            max_depth: 64,
            max_items: usize::MAX,
            max_blob: 32,
        }
    }
}

pub fn to_text(value: Value<'_>, symtab: &SymbolTable, opts: TextOptions) -> String {
    let mut out = String::new();
    write_value(&mut out, value, symtab, opts, 0);
    out
}

fn symbol(out: &mut String, id: u32, symtab: &SymbolTable) {
    let name = symtab.name(id);
    let plain = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b == b'.');
    if plain {
        out.push_str(&name);
    } else {
        let _ = write!(out, "'{}'", name.replace('\'', "\\'"));
    }
}

fn write_value(out: &mut String, value: Value<'_>, symtab: &SymbolTable, opts: TextOptions, depth: usize) {
    if depth > opts.max_depth {
        out.push('…');
        return;
    }
    match value {
        Value::Null(_) => out.push_str("null"),
        Value::Bool(b) => out.push_str(if b { "true" } else { "false" }),
        Value::Int(i) => {
            let _ = write!(out, "{i}");
        }
        Value::Float(f) => {
            let _ = write!(out, "{f:?}e0");
        }
        Value::Decimal(d) => {
            let _ = write!(out, "{}d{}", d.coefficient, d.exponent);
        }
        Value::Timestamp(t) => {
            let _ = write!(out, "{t:?}");
        }
        Value::Symbol(id) => symbol(out, id, symtab),
        Value::String(s) => {
            let _ = write!(out, "{s:?}");
        }
        Value::Clob(b) | Value::Blob(b) => {
            out.push_str("{{");
            for byte in b.iter().take(opts.max_blob) {
                let _ = write!(out, "{byte:02x}");
            }
            if b.len() > opts.max_blob {
                let _ = write!(out, "… {} bytes", b.len());
            }
            out.push_str("}}");
        }
        Value::List(seq) | Value::SExp(seq) => {
            let (open, close, sep) = if matches!(value, Value::List(_)) {
                ('[', ']', ", ")
            } else {
                ('(', ')', " ")
            };
            out.push(open);
            for (i, item) in seq.iter().enumerate() {
                if i > 0 {
                    out.push_str(sep);
                }
                if i >= opts.max_items {
                    out.push('…');
                    break;
                }
                match item {
                    Ok(v) => write_value(out, v, symtab, opts, depth + 1),
                    Err(e) => {
                        let _ = write!(out, "<error: {e}>");
                    }
                }
            }
            out.push(close);
        }
        Value::Struct(st) => {
            out.push('{');
            for (i, field) in st.fields().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                if i >= opts.max_items {
                    out.push('…');
                    break;
                }
                match field {
                    Ok((k, v)) => {
                        symbol(out, k, symtab);
                        out.push_str(": ");
                        write_value(out, v, symtab, opts, depth + 1);
                    }
                    Err(e) => {
                        let _ = write!(out, "<error: {e}>");
                    }
                }
            }
            out.push('}');
        }
        Value::Annotated(a) => {
            for ann in a.annotations() {
                match ann {
                    Ok(id) => symbol(out, id, symtab),
                    Err(_) => out.push_str("<error>"),
                }
                out.push_str("::");
            }
            match a.value() {
                Ok(v) => write_value(out, v, symtab, opts, depth + 1),
                Err(e) => {
                    let _ = write!(out, "<error: {e}>");
                }
            }
        }
    }
}
