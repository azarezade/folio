//! Lazy, zero-copy Ion values.
//!
//! Scalars are decoded when their header is read; containers keep a borrowed
//! slice and decode children only while they are iterated, so skipped fields
//! cost nothing. Behaviour follows `kfxlib/ion_binary.py` (`deserialize_value`).

use crate::binary::{Cursor, signed_int, uint};
use crate::{Error, Result, SIGNATURE, SymbolId};

const VERSION_MARKER: u8 = 0xe0;
const VARIABLE_LEN: u8 = 14;
const NULL_FLAG: u8 = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IonType {
    Null,
    Bool,
    Int,
    Float,
    Decimal,
    Timestamp,
    Symbol,
    String,
    Clob,
    Blob,
    List,
    SExp,
    Struct,
    Annotation,
    Reserved,
}

impl IonType {
    fn from_signature(sig: u8) -> IonType {
        use IonType::*;
        [
            Null, Bool, Int, Int, Float, Decimal, Timestamp, Symbol, String, Clob, Blob, List, SExp, Struct,
            Annotation, Reserved,
        ][usize::from(sig & 0x0f)]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decimal {
    pub coefficient: i128,
    pub exponent: i64,
}

/// Timestamp fields as stored; precision is given by which fields are present.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Timestamp {
    /// `None` is the unknown offset (-00:00).
    pub offset_minutes: Option<i64>,
    pub year: u64,
    pub month: Option<u64>,
    pub day: Option<u64>,
    pub hour: Option<u64>,
    pub minute: Option<u64>,
    pub second: Option<u64>,
    /// Fraction of a second as `coefficient × 10^exponent`.
    pub fraction: Option<(i128, i64)>,
}

#[derive(Clone, Copy, Debug)]
pub enum Value<'a> {
    /// A null, typed or not. kfxlib turns every typed null into a plain `None`.
    Null(IonType),
    Bool(bool),
    Int(i128),
    Float(f64),
    Decimal(Decimal),
    Timestamp(Timestamp),
    Symbol(SymbolId),
    String(&'a str),
    Clob(&'a [u8]),
    Blob(&'a [u8]),
    List(Seq<'a>),
    SExp(Seq<'a>),
    Struct(Struct<'a>),
    Annotated(Annotated<'a>),
}

impl<'a> Value<'a> {
    pub fn ion_type(&self) -> IonType {
        match self {
            Value::Null(_) => IonType::Null,
            Value::Bool(_) => IonType::Bool,
            Value::Int(_) => IonType::Int,
            Value::Float(_) => IonType::Float,
            Value::Decimal(_) => IonType::Decimal,
            Value::Timestamp(_) => IonType::Timestamp,
            Value::Symbol(_) => IonType::Symbol,
            Value::String(_) => IonType::String,
            Value::Clob(_) => IonType::Clob,
            Value::Blob(_) => IonType::Blob,
            Value::List(_) => IonType::List,
            Value::SExp(_) => IonType::SExp,
            Value::Struct(_) => IonType::Struct,
            Value::Annotated(_) => IonType::Annotation,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null(_))
    }

    /// The value inside any annotation wrappers.
    pub fn unannotated(self) -> Result<Value<'a>> {
        let mut v = self;
        while let Value::Annotated(a) = v {
            v = a.value()?;
        }
        Ok(v)
    }

    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Value::Int(i) => i64::try_from(i).ok(),
            _ => None,
        }
    }

    /// Ints, floats and decimals as `f64` (kfxlib reads e.g. `nmdl.thickness` with `float()`).
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Value::Int(i) => Some(i as f64),
            Value::Float(f) => Some(f),
            Value::Decimal(d) => Some(d.coefficient as f64 * 10f64.powi(d.exponent.clamp(-400, 400) as i32)),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_symbol(&self) -> Option<SymbolId> {
        match *self {
            Value::Symbol(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&'a str> {
        match *self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&'a [u8]> {
        match *self {
            Value::Blob(b) | Value::Clob(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_struct(&self) -> Option<Struct<'a>> {
        match *self {
            Value::Struct(s) => Some(s),
            _ => None,
        }
    }

    /// Lists and s-expressions.
    pub fn as_seq(&self) -> Option<Seq<'a>> {
        match *self {
            Value::List(s) | Value::SExp(s) => Some(s),
            _ => None,
        }
    }
}

/// The elements of a list or s-expression. NOP padding is skipped.
#[derive(Clone, Copy, Debug)]
pub struct Seq<'a> {
    data: &'a [u8],
}

impl<'a> Seq<'a> {
    pub fn iter(&self) -> SeqIter<'a> {
        SeqIter {
            cursor: Cursor::new(self.data),
        }
    }

    pub fn encoded_len(&self) -> usize {
        self.data.len()
    }
}

impl<'a> IntoIterator for Seq<'a> {
    type Item = Result<Value<'a>>;
    type IntoIter = SeqIter<'a>;

    fn into_iter(self) -> SeqIter<'a> {
        self.iter()
    }
}

pub struct SeqIter<'a> {
    cursor: Cursor<'a>,
}

impl<'a> Iterator for SeqIter<'a> {
    type Item = Result<Value<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.cursor.is_empty() {
            match read_value(&mut self.cursor) {
                Ok(Some(v)) => return Some(Ok(v)),
                Ok(None) => continue,
                Err(e) => {
                    self.cursor = Cursor::new(&[]);
                    return Some(Err(e));
                }
            }
        }
        None
    }
}

/// A struct's fields, in stored order. NOP-valued fields are skipped.
#[derive(Clone, Copy, Debug)]
pub struct Struct<'a> {
    data: &'a [u8],
}

impl<'a> Struct<'a> {
    pub fn fields(&self) -> StructIter<'a> {
        StructIter {
            cursor: Cursor::new(self.data),
        }
    }

    /// The value of field `id`. As in kfxlib's dict, a repeated field keeps its last value.
    pub fn get(&self, id: SymbolId) -> Result<Option<Value<'a>>> {
        let mut found = None;
        for field in self.fields() {
            let (k, v) = field?;
            if k == id {
                found = Some(v);
            }
        }
        Ok(found)
    }

    pub fn contains(&self, id: SymbolId) -> Result<bool> {
        Ok(self.get(id)?.is_some())
    }

    pub fn is_empty(&self) -> bool {
        self.fields().next().is_none()
    }

    pub fn encoded_len(&self) -> usize {
        self.data.len()
    }
}

pub struct StructIter<'a> {
    cursor: Cursor<'a>,
}

impl<'a> StructIter<'a> {
    fn field(&mut self) -> Result<Option<(SymbolId, Value<'a>)>> {
        let id = symbol_id(self.cursor.varuint()?)?;
        Ok(read_value(&mut self.cursor)?.map(|v| (id, v)))
    }
}

impl<'a> Iterator for StructIter<'a> {
    type Item = Result<(SymbolId, Value<'a>)>;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.cursor.is_empty() {
            match self.field() {
                Ok(Some(f)) => return Some(Ok(f)),
                Ok(None) => continue,
                Err(e) => {
                    self.cursor = Cursor::new(&[]);
                    return Some(Err(e));
                }
            }
        }
        None
    }
}

/// An annotation wrapper: one or more symbol annotations on a single value.
#[derive(Clone, Copy, Debug)]
pub struct Annotated<'a> {
    annotations: &'a [u8],
    value: &'a [u8],
}

impl<'a> Annotated<'a> {
    pub fn annotations(&self) -> impl Iterator<Item = Result<SymbolId>> + use<'a> {
        let mut cursor = Cursor::new(self.annotations);
        std::iter::from_fn(move || (!cursor.is_empty()).then(|| symbol_id(cursor.varuint()?)))
    }

    pub fn first_annotation(&self) -> Result<SymbolId> {
        self.annotations()
            .next()
            .unwrap_or(Err(Error::invalid("annotation wrapper has no annotations")))
    }

    pub fn has_annotation(&self, id: SymbolId) -> bool {
        self.annotations().any(|a| a == Ok(id))
    }

    pub fn value(&self) -> Result<Value<'a>> {
        let mut cursor = Cursor::new(self.value);
        read_value(&mut cursor)?.ok_or(Error::invalid("annotation wraps a NOP"))
    }
}

fn symbol_id(id: u64) -> Result<SymbolId> {
    SymbolId::try_from(id).map_err(|_| Error::invalid("symbol id too large"))
}

/// Reads one value. `Ok(None)` is NOP padding.
pub(crate) fn read_value<'a>(cursor: &mut Cursor<'a>) -> Result<Option<Value<'a>>> {
    let descriptor = cursor.u8()?;
    if descriptor == VERSION_MARKER {
        return Err(Error::invalid("unexpected Ion version marker within data"));
    }
    let sig = descriptor >> 4;
    let flag = descriptor & 0x0f;
    let ion_type = IonType::from_signature(sig);

    if flag == NULL_FLAG {
        return Ok(Some(Value::Null(ion_type)));
    }

    let length = |cursor: &mut Cursor<'a>, flag: u8| -> Result<usize> {
        if flag == VARIABLE_LEN {
            cursor.varuint_usize()
        } else {
            Ok(usize::from(flag))
        }
    };

    let value = match ion_type {
        IonType::Null => {
            let n = length(cursor, flag)?;
            cursor.take(n)?;
            return Ok(None);
        }
        IonType::Bool => match flag {
            0 => Value::Bool(false),
            1 => Value::Bool(true),
            _ => return Err(Error::invalid("bad bool flag")),
        },
        IonType::Struct => {
            // flag 1 marks a sorted struct; kfxlib treats it as variable-length
            let flag = if flag == 1 { VARIABLE_LEN } else { flag };
            let n = length(cursor, flag)?;
            Value::Struct(Struct { data: cursor.take(n)? })
        }
        _ => {
            let n = length(cursor, flag)?;
            let data = cursor.take(n)?;
            match (ion_type, sig) {
                (IonType::Int, 2) => Value::Int(i128::from(uint(data)?)),
                (IonType::Int, _) => {
                    if data.is_empty() {
                        return Err(Error::invalid("negative int has no data"));
                    }
                    Value::Int(-i128::from(uint(data)?))
                }
                (IonType::Float, _) => Value::Float(match data.len() {
                    0 => 0.0,
                    4 => f64::from(f32::from_be_bytes(data.try_into().unwrap())),
                    8 => f64::from_be_bytes(data.try_into().unwrap()),
                    _ => return Err(Error::invalid("bad float length")),
                }),
                (IonType::Decimal, _) => Value::Decimal(decimal(data)?),
                (IonType::Timestamp, _) => Value::Timestamp(timestamp(data)?),
                (IonType::Symbol, _) => Value::Symbol(symbol_id(uint(data)?)?),
                (IonType::String, _) => {
                    Value::String(std::str::from_utf8(data).map_err(|_| Error::invalid("string is not UTF-8"))?)
                }
                (IonType::Clob, _) => Value::Clob(data),
                (IonType::Blob, _) => Value::Blob(data),
                (IonType::List, _) => Value::List(Seq { data }),
                (IonType::SExp, _) => Value::SExp(Seq { data }),
                (IonType::Annotation, _) => Value::Annotated(annotated(data)?),
                _ => return Err(Error::invalid("reserved type code")),
            }
        }
    };
    Ok(Some(value))
}

fn decimal(data: &[u8]) -> Result<Decimal> {
    if data.is_empty() {
        return Ok(Decimal {
            coefficient: 0,
            exponent: 0,
        });
    }
    let mut cursor = Cursor::new(data);
    let exponent = cursor.varint(false)?.unwrap_or(0);
    let coefficient = signed_int(cursor.rest())?;
    Ok(Decimal { coefficient, exponent })
}

fn timestamp(data: &[u8]) -> Result<Timestamp> {
    let mut c = Cursor::new(data);
    let offset_minutes = c.varint(true)?;
    let next = |c: &mut Cursor| -> Result<Option<u64>> { if c.is_empty() { Ok(None) } else { c.varuint().map(Some) } };
    let year = next(&mut c)?.ok_or(Error::Eof)?;
    let month = next(&mut c)?;
    let day = next(&mut c)?;
    let hour = next(&mut c)?;
    let minute = next(&mut c)?;
    let second = next(&mut c)?;
    let fraction = if c.is_empty() {
        None
    } else {
        let exponent = c.varint(false)?.unwrap_or(0);
        Some((signed_int(c.rest())?, exponent))
    };
    Ok(Timestamp {
        offset_minutes,
        year,
        month,
        day,
        hour,
        minute,
        second,
        fraction,
    })
}

fn annotated(data: &[u8]) -> Result<Annotated<'_>> {
    let mut cursor = Cursor::new(data);
    let n = cursor.varuint_usize()?;
    let annotations = cursor.take(n)?;
    if annotations.is_empty() {
        return Err(Error::invalid("annotation wrapper has no annotations"));
    }
    let value = cursor.rest();
    let wrapped = Annotated { annotations, value };
    for a in wrapped.annotations() {
        a?;
    }
    // exactly one value, no excess data
    let mut inner = Cursor::new(value);
    if read_value(&mut inner)?.is_none() {
        return Err(Error::invalid("annotation wraps a NOP"));
    }
    if !inner.is_empty() {
        return Err(Error::invalid("annotation wrapper has excess data"));
    }
    Ok(wrapped)
}

fn values(data: &[u8]) -> impl Iterator<Item = Result<Value<'_>>> {
    let mut cursor = Cursor::new(data);
    let mut started = false;
    std::iter::from_fn(move || {
        if !started {
            started = true;
            if cursor.take(4).ok() != Some(&SIGNATURE[..]) {
                cursor = Cursor::new(&[]);
                return Some(Err(Error::invalid("missing Ion version marker")));
            }
        }
        loop {
            if cursor.is_empty() {
                return None;
            }
            let item = if cursor.peek() == Some(VERSION_MARKER) {
                match cursor.take(4) {
                    Ok(sig) if sig == SIGNATURE => continue,
                    _ => Err(Error::invalid("bad embedded Ion version marker")),
                }
            } else {
                match read_value(&mut cursor) {
                    Ok(Some(v)) => Ok(v),
                    Ok(None) => continue,
                    Err(e) => Err(e),
                }
            };
            if item.is_err() {
                cursor = Cursor::new(&[]);
            }
            return Some(item);
        }
    })
}

/// All top-level values of an Ion binary document (which starts with `E0 01 00 EA`).
pub fn read_all(data: &[u8]) -> Result<Vec<Value<'_>>> {
    values(data).collect()
}

/// The single top-level value of an Ion binary document, as every KDF payload holds.
pub fn read_single(data: &[u8]) -> Result<Value<'_>> {
    let mut it = values(data);
    let first = it
        .next()
        .ok_or(Error::invalid("expected one Ion value, found none"))??;
    match it.next() {
        None => Ok(first),
        Some(Err(e)) => Err(e),
        Some(Ok(_)) => Err(Error::invalid("expected one Ion value, found several")),
    }
}

/// The annotations on a document's first value, read from just the start of the
/// document. `Ok(None)` if that value is not annotated; `Err(Eof)` if `head` is
/// too short to hold them.
pub fn peek_annotations(head: &[u8]) -> Result<Option<Vec<SymbolId>>> {
    let mut cursor = Cursor::new(head);
    loop {
        if cursor.peek() == Some(VERSION_MARKER) {
            if cursor.take(4)? != SIGNATURE {
                return Err(Error::invalid("bad Ion version marker"));
            }
            continue;
        }
        let descriptor = cursor.u8()?;
        let (sig, flag) = (descriptor >> 4, descriptor & 0x0f);
        if sig == 0 && flag != NULL_FLAG {
            // NOP padding before the value
            let n = if flag == VARIABLE_LEN {
                cursor.varuint_usize()?
            } else {
                usize::from(flag)
            };
            cursor.take(n)?;
            continue;
        }
        if sig != 14 || flag == NULL_FLAG {
            return Ok(None);
        }
        if flag == VARIABLE_LEN {
            cursor.varuint()?;
        }
        let n = cursor.varuint_usize()?;
        let mut annotations = Cursor::new(cursor.take(n)?);
        let mut ids = Vec::new();
        while !annotations.is_empty() {
            ids.push(symbol_id(annotations.varuint()?)?);
        }
        if ids.is_empty() {
            return Err(Error::invalid("annotation wrapper has no annotations"));
        }
        return Ok(Some(ids));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(body: &[u8]) -> Vec<u8> {
        [&SIGNATURE[..], body].concat()
    }

    #[test]
    fn scalars() {
        let d = doc(&[0x21, 0x05]);
        assert_eq!(read_single(&d).unwrap().as_i64(), Some(5));
        let d = doc(&[0x31, 0x05]);
        assert_eq!(read_single(&d).unwrap().as_i64(), Some(-5));
        let d = doc(&[0x83, b'a', b'b', b'c']);
        assert_eq!(read_single(&d).unwrap().as_str(), Some("abc"));
        let d = doc(&[0x71, 0x0a]);
        assert_eq!(read_single(&d).unwrap().as_symbol(), Some(10));
        let d = doc(&[0x11]);
        assert_eq!(read_single(&d).unwrap().as_bool(), Some(true));
        let d = doc(&[0x2f]);
        assert!(read_single(&d).unwrap().is_null());
        let d = doc(&[0x48, 0x40, 0x09, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x18]);
        assert_eq!(read_single(&d).unwrap().as_f64(), Some(std::f64::consts::PI));
    }

    #[test]
    fn decimal_value() {
        // 2520.0 = 25200 × 10^-1
        let d = doc(&[0x53, 0xc1, 0x62, 0x70]);
        let Value::Decimal(v) = read_single(&d).unwrap() else {
            panic!()
        };
        assert_eq!(
            v,
            Decimal {
                coefficient: 25200,
                exponent: -1
            }
        );
        assert_eq!(read_single(&d).unwrap().as_f64(), Some(2520.0));
    }

    #[test]
    fn containers_and_annotations() {
        // $260::{ $174: "a", $141: [1, NOP, 2] }
        #[rustfmt::skip]
        let d = doc(&[
            0xee, 0x90,             // annotation wrapper, 16 bytes
            0x82, 0x02, 0x84,       // 2 bytes of annotations: VarUInt 260
            0xdc,                   // struct, 12 bytes
            0x01, 0xae, 0x81, b'a', // $174: "a"
            0x01, 0x8d, 0xb5,       // $141: list, 5 bytes
            0x21, 0x01, 0x00, 0x21, 0x02,
        ]);
        assert_eq!(peek_annotations(&d).unwrap(), Some(vec![260]));
        assert_eq!(peek_annotations(&d[..9]).unwrap(), Some(vec![260]));
        let v = read_single(&d).unwrap();
        let Value::Annotated(a) = v else { panic!("{v:?}") };
        assert_eq!(a.first_annotation(), Ok(260));
        let s = a.value().unwrap().as_struct().unwrap();
        assert_eq!(s.get(174).unwrap().unwrap().as_str(), Some("a"));
        let items: Vec<i64> = s
            .get(141)
            .unwrap()
            .unwrap()
            .as_seq()
            .unwrap()
            .iter()
            .map(|v| v.unwrap().as_i64().unwrap())
            .collect();
        assert_eq!(items, vec![1, 2]);
    }

    #[test]
    fn errors_not_panics() {
        assert!(read_single(&[]).is_err());
        assert!(read_single(&doc(&[0xd5, 0x81])).is_err()); // truncated struct
        assert!(
            read_single(&doc(&[0xd2, 0x81, 0x21]))
                .unwrap()
                .as_struct()
                .unwrap()
                .fields()
                .any(|f| f.is_err())
        );
        assert!(read_single(&doc(&[0xe4, 0x81, 0x84, 0x20, 0x20])).is_err()); // excess data in wrapper
        assert!(read_single(&doc(&[0x21, 0x01, 0x21, 0x02])).is_err()); // two values
        assert!(read_single(&doc(&[0xf0])).is_err());
    }
}
