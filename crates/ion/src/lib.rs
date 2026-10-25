//! Ion binary reader and the YJ_symbols shared symbol table.
//!
//! A port of `kfxlib/ion_binary.py` and `ion_symbol_table.py` (reading), plus a
//! minimal writer for synthetic values.
//! Values borrow from the input and decode lazily; see [`value`].

mod binary;
pub mod diag;
pub mod symtab;
pub mod text;
pub mod value;
pub mod write;

pub use diag::Diagnostics;
pub use symtab::SymbolTable;
pub use value::{Annotated, IonType, Seq, Struct, Value, peek_annotations, read_all, read_single};

pub type SymbolId = u32;

/// Ion 1.0 binary version marker, which starts every document.
pub const SIGNATURE: [u8; 4] = [0xe0, 0x01, 0x00, 0xea];

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("Ion data ends unexpectedly")]
    Eof,
    #[error("invalid Ion data: {0}")]
    Invalid(&'static str),
}

impl Error {
    pub(crate) fn invalid(msg: &'static str) -> Error {
        Error::Invalid(msg)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Visits every value under `value` (for validation and fuzzing). Fails on the
/// first decoding error or when nesting exceeds `max_depth`.
pub fn walk(value: Value<'_>, max_depth: usize, visit: &mut impl FnMut(&Value<'_>)) -> Result<()> {
    if max_depth == 0 {
        return Err(Error::invalid("nesting too deep"));
    }
    visit(&value);
    match value {
        Value::List(seq) | Value::SExp(seq) => {
            for item in seq {
                walk(item?, max_depth - 1, visit)?;
            }
        }
        Value::Struct(st) => {
            for field in st.fields() {
                walk(field?.1, max_depth - 1, visit)?;
            }
        }
        Value::Annotated(a) => {
            for ann in a.annotations() {
                ann?;
            }
            walk(a.value()?, max_depth - 1, visit)?;
        }
        _ => {}
    }
    Ok(())
}
