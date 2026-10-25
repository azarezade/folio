//! A minimal Ion binary writer, for the few synthetic values the readers build
//! themselves (e.g. the container-info fragment kfxlib adds to every KDF).

use crate::{SIGNATURE, SymbolId};

#[derive(Clone, Debug, PartialEq)]
pub enum Owned {
    Null,
    Bool(bool),
    Int(i64),
    String(String),
    Symbol(SymbolId),
    List(Vec<Owned>),
    Struct(Vec<(SymbolId, Owned)>),
    Annotated(Vec<SymbolId>, Box<Owned>),
}

/// Encodes `value` as a complete Ion document (version marker + one value).
pub fn to_document(value: &Owned) -> Vec<u8> {
    let mut out = SIGNATURE.to_vec();
    encode(value, &mut out);
    out
}

fn varuint(mut v: u64, out: &mut Vec<u8>) {
    let mut bytes = vec![(v & 0x7f) as u8 | 0x80];
    v >>= 7;
    while v > 0 {
        bytes.push((v & 0x7f) as u8);
        v >>= 7;
    }
    out.extend(bytes.iter().rev());
}

fn uint_bytes(v: u64) -> Vec<u8> {
    let b = v.to_be_bytes();
    let start = b.iter().position(|&x| x != 0).unwrap_or(8);
    b[start..].to_vec()
}

fn header(sig: u8, body: &[u8], out: &mut Vec<u8>) {
    if body.len() < 14 {
        out.push(sig << 4 | body.len() as u8);
    } else {
        out.push(sig << 4 | 14);
        varuint(body.len() as u64, out);
    }
    out.extend_from_slice(body);
}

fn encode(value: &Owned, out: &mut Vec<u8>) {
    match value {
        Owned::Null => out.push(0x0f),
        Owned::Bool(b) => out.push(0x10 | u8::from(*b)),
        Owned::Int(i) => header(if *i < 0 { 3 } else { 2 }, &uint_bytes(i.unsigned_abs()), out),
        Owned::String(s) => header(8, s.as_bytes(), out),
        Owned::Symbol(id) => header(7, &uint_bytes(u64::from(*id)), out),
        Owned::List(items) => {
            let mut body = Vec::new();
            for item in items {
                encode(item, &mut body);
            }
            header(11, &body, out);
        }
        Owned::Struct(fields) => {
            let mut body = Vec::new();
            for (k, v) in fields {
                varuint(u64::from(*k), &mut body);
                encode(v, &mut body);
            }
            // a 1-byte length would read as the "sorted" flag
            if body.len() == 1 {
                out.push(0xde);
                varuint(1, out);
                out.extend_from_slice(&body);
            } else {
                header(13, &body, out);
            }
        }
        Owned::Annotated(annotations, inner) => {
            let mut ann = Vec::new();
            for a in annotations {
                varuint(u64::from(*a), &mut ann);
            }
            let mut body = Vec::new();
            varuint(ann.len() as u64, &mut body);
            body.extend_from_slice(&ann);
            encode(inner, &mut body);
            header(14, &body, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Value, read_single};

    #[test]
    fn round_trip() {
        let v = Owned::Annotated(
            vec![270],
            Box::new(Owned::Struct(vec![
                (587, Owned::String(String::new())),
                (161, Owned::String("KPF".into())),
                (5, Owned::Int(-300)),
                (6, Owned::List(vec![Owned::Symbol(900), Owned::Bool(true), Owned::Null])),
            ])),
        );
        let doc = to_document(&v);
        let Value::Annotated(a) = read_single(&doc).unwrap() else {
            panic!()
        };
        assert_eq!(a.first_annotation(), Ok(270));
        let st = a.value().unwrap().as_struct().unwrap();
        assert_eq!(st.get(161).unwrap().unwrap().as_str(), Some("KPF"));
        assert_eq!(st.get(587).unwrap().unwrap().as_str(), Some(""));
        assert_eq!(st.get(5).unwrap().unwrap().as_i64(), Some(-300));
        let list: Vec<_> = st
            .get(6)
            .unwrap()
            .unwrap()
            .as_seq()
            .unwrap()
            .iter()
            .map(Result::unwrap)
            .collect();
        assert_eq!(list[0].as_symbol(), Some(900));
        assert!(list[2].is_null());
    }
}
