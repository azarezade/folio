//! Ion symbol tables: the system table, the YJ_symbols shared table and local symbols.
//!
//! Ported from `kfxlib/ion_symbol_table.py` (`LocalSymbolTable`). kfxlib's
//! `YJ_symbols` catalog (`yj_symbol_catalog.py`, version 10) names its 825
//! symbols `$10` … `$834`: the name of a shared symbol is always `$` + its id.
//! So no catalog data is needed, only its size. Ids past the end of the catalog
//! (newer firmware) get the same `$N` names, as kfxlib's padding does.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::diag::Diagnostics;
use crate::value::Value;
use crate::{Result, SymbolId};

pub const SYSTEM_SYMBOLS: [&str; 9] = [
    "$ion",
    "$ion_1_0",
    "$ion_symbol_table",
    "name",
    "version",
    "imports",
    "symbols",
    "max_id",
    "$ion_shared_symbol_table",
];

/// System symbol ids.
pub mod sys {
    use crate::SymbolId;
    pub const ION_SYMBOL_TABLE: SymbolId = 3;
    pub const NAME: SymbolId = 4;
    pub const VERSION: SymbolId = 5;
    pub const IMPORTS: SymbolId = 6;
    pub const SYMBOLS: SymbolId = 7;
    pub const MAX_ID: SymbolId = 8;
}

pub const YJ_SYMBOLS_NAME: &str = "YJ_symbols";
pub const YJ_SYMBOLS_VERSION: i64 = 10;
/// Number of symbols in kfxlib's YJ_symbols catalog.
pub const YJ_SYMBOLS_LEN: u32 = 825;

#[derive(Clone, Debug)]
pub struct SymbolTable {
    /// Highest id covered by the system and shared tables.
    shared_max: SymbolId,
    locals: Vec<Option<Box<str>>>,
    local_ids: HashMap<Box<str>, SymbolId>,
}

impl Default for SymbolTable {
    fn default() -> Self {
        SymbolTable::new()
    }
}

impl SymbolTable {
    /// Only the system symbols.
    pub fn new() -> Self {
        SymbolTable {
            shared_max: SYSTEM_SYMBOLS.len() as SymbolId,
            locals: Vec::new(),
            local_ids: HashMap::new(),
        }
    }

    /// System symbols plus YJ_symbols truncated or padded to `yj_max_id` symbols.
    pub fn with_yj_symbols(yj_max_id: u32, diag: &mut Diagnostics) -> Self {
        let mut t = SymbolTable::new();
        t.import_shared(YJ_SYMBOLS_NAME, None, Some(yj_max_id), diag);
        t
    }

    /// Imports a shared table. Must happen before any local symbol is added.
    pub fn import_shared(&mut self, name: &str, version: Option<i64>, max_id: Option<u32>, diag: &mut Diagnostics) {
        assert!(self.locals.is_empty(), "shared symbols imported after local symbols");
        if name == "$ion" {
            return;
        }
        let known_len = if name == YJ_SYMBOLS_NAME {
            if version.is_some_and(|v| v != YJ_SYMBOLS_VERSION) && max_id.is_none() {
                diag.error("symtab.version", || {
                    format!("import of {name} version {version:?} without max_id")
                });
            }
            YJ_SYMBOLS_LEN
        } else {
            diag.error("symtab.unknown_table", || {
                format!("imported shared symbol table {name} is unknown")
            });
            0
        };
        let max_id = max_id.unwrap_or(known_len);
        if max_id > known_len && known_len > 0 {
            diag.warning("symtab.beyond_catalog", || {
                format!("import of {name} max_id {max_id} exceeds known table size {known_len}")
            });
        }
        self.shared_max = self.shared_max.saturating_add(max_id);
    }

    pub fn add_local(&mut self, name: Option<&str>) -> SymbolId {
        let id = self.shared_max + 1 + self.locals.len() as SymbolId;
        if let Some(name) = name {
            self.local_ids.entry(name.into()).or_insert(id);
        }
        self.locals.push(name.map(Into::into));
        id
    }

    /// First id of the local symbols.
    pub fn local_min_id(&self) -> SymbolId {
        self.shared_max + 1
    }

    pub fn max_id(&self) -> SymbolId {
        self.shared_max + self.locals.len() as SymbolId
    }

    pub fn local_symbols(&self) -> impl Iterator<Item = Option<&str>> {
        self.locals.iter().map(|s| s.as_deref())
    }

    /// The symbol's text as kfxlib prints it: undefined ids become `$N`.
    pub fn name(&self, id: SymbolId) -> Cow<'_, str> {
        if (1..=SYSTEM_SYMBOLS.len() as SymbolId).contains(&id) {
            return Cow::Borrowed(SYSTEM_SYMBOLS[id as usize - 1]);
        }
        if id > self.shared_max
            && let Some(Some(name)) = self.locals.get((id - self.shared_max - 1) as usize)
        {
            return Cow::Borrowed(name);
        }
        Cow::Owned(format!("${id}"))
    }

    /// The id of a symbol given by text, including `$N` forms.
    pub fn id(&self, name: &str) -> Option<SymbolId> {
        if let Some(n) = name
            .strip_prefix('$')
            .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            return n.parse().ok();
        }
        if let Some(i) = SYSTEM_SYMBOLS.iter().position(|s| *s == name) {
            return Some(i as SymbolId + 1);
        }
        self.local_ids.get(name).copied()
    }

    /// Applies an `$ion_symbol_table` struct (the value under the annotation),
    /// as `LocalSymbolTable.create` does.
    pub fn apply_symbol_table(&mut self, value: Value<'_>, diag: &mut Diagnostics) -> Result<()> {
        let Some(st) = value.unannotated()?.as_struct() else {
            diag.error("symtab.not_struct", || "$ion_symbol_table is not a struct".into());
            return Ok(());
        };
        match st.get(sys::IMPORTS)? {
            Some(Value::Symbol(sys::ION_SYMBOL_TABLE)) => {} // append to the current table
            Some(Value::Symbol(_)) => diag.error("symtab.imports", || "unexpected imports symbol".into()),
            Some(Value::List(imports)) => {
                *self = SymbolTable::new();
                for import in imports {
                    let Some(import) = import?.as_struct() else {
                        continue;
                    };
                    let name = import.get(sys::NAME)?.and_then(|v| v.as_str()).unwrap_or("");
                    let version = import
                        .get(sys::VERSION)?
                        .and_then(|v| v.as_i64())
                        .filter(|&v| v != 0)
                        .or(Some(1));
                    let max_id = import
                        .get(sys::MAX_ID)?
                        .and_then(|v| v.as_i64())
                        .map(|m| m.clamp(0, u32::MAX as i64) as u32);
                    self.import_shared(name, version, max_id, diag);
                }
            }
            _ => *self = SymbolTable::new(),
        }
        if let Some(symbols) = st.get(sys::SYMBOLS)?
            && let Some(symbols) = symbols.unannotated()?.as_seq()
        {
            for s in symbols {
                let s = s?.unannotated()?;
                match s {
                    Value::String(name) if !name.is_empty() => {
                        self.add_local(Some(name));
                    }
                    Value::Null(_) => {
                        self.add_local(None);
                    }
                    _ => {
                        diag.error("symtab.symbol_type", || {
                            format!("imported symbol is {:?}, treating as null", s.ion_type())
                        });
                        self.add_local(None);
                    }
                }
            }
        }
        if let Some(expected) = st.get(sys::MAX_ID)?.and_then(|v| v.as_i64())
            && expected != i64::from(self.max_id())
        {
            let found = self.max_id();
            diag.error("symtab.max_id", || {
                format!("symbol table max_id expected {expected}, found {found}")
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        let mut d = Diagnostics::default();
        let mut t = SymbolTable::with_yj_symbols(825, &mut d);
        assert_eq!(t.local_min_id(), 835);
        let id = t.add_local(Some("nmdl.canvas_width"));
        assert_eq!(id, 835);
        assert_eq!(t.name(3), "$ion_symbol_table");
        assert_eq!(t.name(260), "$260");
        assert_eq!(t.name(835), "nmdl.canvas_width");
        assert_eq!(t.name(900), "$900");
        assert_eq!(t.id("nmdl.canvas_width"), Some(835));
        assert_eq!(t.id("$260"), Some(260));
        assert_eq!(t.id("max_id"), Some(8));
        assert!(d.is_empty());

        let padded = SymbolTable::with_yj_symbols(900, &mut d);
        assert_eq!(padded.local_min_id(), 910);
        assert_eq!(d.count("symtab.beyond_catalog"), 1);
    }
}
