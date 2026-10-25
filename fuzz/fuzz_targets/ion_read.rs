//! Any input: the Ion reader must return errors, never panic or overflow the stack.
#![no_main]

use libfuzzer_sys::fuzz_target;
use folio_ion::text::{TextOptions, to_text};
use folio_ion::{Diagnostics, SIGNATURE, SymbolTable, peek_annotations, read_all, walk};

fuzz_target!(|data: &[u8]| {
    // Most inputs should get past the version marker.
    let doc = [&SIGNATURE[..], data].concat();
    let _ = peek_annotations(&doc);
    let mut diag = Diagnostics::default();
    let mut symtab = SymbolTable::with_yj_symbols(825, &mut diag);
    if let Ok(values) = read_all(&doc) {
        for v in values {
            let _ = walk(v, 256, &mut |x| {
                let _ = (x.as_f64(), x.as_str(), x.as_bytes(), x.as_i64());
            });
            let _ = to_text(v, &symtab, TextOptions { max_depth: 64, max_items: 64, max_blob: 8 });
            let _ = symtab.apply_symbol_table(v, &mut diag);
        }
    }
});
