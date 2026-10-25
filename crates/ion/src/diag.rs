//! Aggregated diagnostics.
//!
//! Real notebooks produce thousands of identical, harmless warnings
//! (docs/format.md §5). Each kind of message is counted once with its first
//! example, instead of being logged line by line.

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Known and harmless; shown only on request.
    Debug,
    Warning,
    Error,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Entry {
    pub kind: &'static str,
    pub level: Level,
    pub count: u64,
    pub example: String,
}

#[derive(Clone, Debug, Default)]
pub struct Diagnostics {
    entries: BTreeMap<&'static str, Entry>,
}

impl Diagnostics {
    /// Records one occurrence of `kind`. `message` is only formatted for the first one.
    pub fn add(&mut self, level: Level, kind: &'static str, message: impl FnOnce() -> String) {
        self.entries
            .entry(kind)
            .and_modify(|e| e.count += 1)
            .or_insert_with(|| Entry {
                kind,
                level,
                count: 1,
                example: message(),
            });
    }

    pub fn debug(&mut self, kind: &'static str, message: impl FnOnce() -> String) {
        self.add(Level::Debug, kind, message);
    }

    pub fn warning(&mut self, kind: &'static str, message: impl FnOnce() -> String) {
        self.add(Level::Warning, kind, message);
    }

    pub fn error(&mut self, kind: &'static str, message: impl FnOnce() -> String) {
        self.add(Level::Error, kind, message);
    }

    pub fn merge(&mut self, other: Diagnostics) {
        for (kind, e) in other.entries {
            self.entries.entry(kind).and_modify(|x| x.count += e.count).or_insert(e);
        }
    }

    pub fn count(&self, kind: &str) -> u64 {
        self.entries.get(kind).map_or(0, |e| e.count)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Entries, most severe first, then most frequent.
    pub fn entries(&self) -> Vec<&Entry> {
        let mut v: Vec<_> = self.entries.values().collect();
        v.sort_by(|a, b| {
            b.level
                .cmp(&a.level)
                .then(b.count.cmp(&a.count))
                .then(a.kind.cmp(b.kind))
        });
        v
    }
}
