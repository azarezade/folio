//! Fragment reachability, as kfxlib's `check_fragment_usage` computes it
//! (`yj_structure.py:718`, `walk_fragment` at `:857`).
//!
//! Starting from the root fragments, every value is walked and symbols under
//! known keys become references to other fragments. kfxlib keeps only the
//! reachable fragments, so its fragment counts are the reachable ones; deleted
//! notebook pages that are still in the database fall away here.

use std::collections::{BTreeSet, HashSet};

use folio_ion::{Diagnostics, SymbolId, Value};

use crate::container::{FragmentKey, Kdf};
use crate::{Result, sym};

const MAX_DEPTH: usize = 512;

#[derive(Clone, Debug, Default)]
pub struct Usage {
    /// Indices into [`Kdf::fragments`] of the fragments kfxlib keeps.
    pub reachable: Vec<usize>,
    /// Referenced keys with no fragment.
    pub missing: Vec<FragmentKey>,
    /// Unreachable fragments kfxlib would report (unreachable pages, stories
    /// and structures of a notebook are dropped silently and not listed).
    pub unreferenced: Vec<usize>,
    /// EIDs referenced but never defined.
    pub undefined_eids: usize,
    /// The first ten of them in sorted order, as kfxlib lists them.
    pub undefined_eid_sample: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctx {
    None,
    Sym(SymbolId),
    /// A non-symbol s-expression head; matches no key.
    Other,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Eid {
    Sym(Box<str>),
    Int(i128),
}

struct Walker<'k> {
    kdf: &'k Kdf,
    ftype: SymbolId,
    kpf_prepub: bool,
    mandatory: BTreeSet<FragmentKey>,
    optional: BTreeSet<FragmentKey>,
    eid_defs: &'k mut HashSet<Eid>,
    eid_refs: &'k mut HashSet<Eid>,
}

impl Walker<'_> {
    fn walk(&mut self, value: Value<'_>, container: Ctx, parent: Ctx, depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(crate::Error::Invalid("fragment nesting too deep".into()));
        }
        match value {
            Value::Annotated(a) => {
                // kfxlib resolves $598:: kfxid annotations to symbols at load time
                if a.first_annotation()? == sym::KFX_ID {
                    match a.value()? {
                        Value::String(s) => return self.symbol(s, container, parent),
                        Value::Int(eid) => {
                            return match i64::try_from(eid).ok().and_then(|e| self.kdf.eid(e)) {
                                Some(name) => self.symbol(name, container, parent),
                                None => {
                                    self.eid(Eid::Int(eid), container);
                                    Ok(())
                                }
                            };
                        }
                        _ => {}
                    }
                }
                self.walk(a.value()?, container, parent, depth + 1)?;
            }
            Value::List(seq) => {
                for item in seq {
                    self.walk(item?, container, parent, depth + 1)?;
                }
            }
            Value::Struct(st) => {
                for field in st.fields() {
                    let (k, v) = field?;
                    self.walk(v, Ctx::Sym(k), container, depth + 1)?;
                }
            }
            Value::SExp(seq) => {
                let mut items = seq.iter();
                let head = match items.next().transpose()? {
                    Some(Value::Symbol(s)) => Ctx::Sym(s),
                    Some(_) => Ctx::Other,
                    None => return Ok(()),
                };
                for item in items {
                    self.walk(item?, head, container, depth + 1)?;
                }
            }
            Value::String(s) => {
                if matches!(container, Ctx::Sym(165 | 636)) {
                    self.symbol(s, container, parent)?;
                }
            }
            Value::Symbol(id) => {
                let name = self.kdf.symtab().name(id);
                self.symbol(&name, container, parent)?;
            }
            Value::Int(i) => self.eid(Eid::Int(i), container),
            _ => {}
        }
        Ok(())
    }

    fn symbol(&mut self, name: &str, container: Ctx, parent: Ctx) -> Result<()> {
        if container == Ctx::Sym(155) || (self.kpf_prepub && container == Ctx::Sym(sym::SECTION_NAME)) {
            self.eid_defs.insert(Eid::Sym(name.into()));
        }

        let own_id = matches!((parent, container), (Ctx::Sym(p), Ctx::Sym(c)) if p == self.ftype && sym::fragment_id_keys(self.ftype).contains(&c));
        if !own_id && let Some(frag_ref) = self.reference(container, parent) {
            let key = |ftype| FragmentKey::new(ftype, Some(name));
            if container == Ctx::Sym(sym::NAME) && matches!(parent, Ctx::Sym(249 | 692)) {
                self.mandatory.insert(key(692));
            } else if container == Ctx::Sym(165) && self.kdf.get(418, Some(name)).is_some() {
                self.mandatory.insert(key(418));
            } else if container == Ctx::Sym(635) {
                self.optional.insert(key(frag_ref));
            } else {
                self.mandatory.insert(key(frag_ref));
            }

            if frag_ref == sym::SECTION {
                let spm = format!("{name}-spm");
                let ad = format!("{name}-ad");
                for (ftype, fid) in [
                    (609, name),
                    (609, &spm),
                    (597, &ad),
                    (597, name),
                    (267, name),
                    (387, name),
                ] {
                    if self.kdf.get(ftype, Some(fid)).is_some() {
                        self.mandatory.insert(FragmentKey::new(ftype, Some(fid)));
                    }
                }
            }
        }

        self.eid(Eid::Sym(name.into()), container);
        Ok(())
    }

    /// The fragment type a symbol under `container` refers to, if any.
    fn reference(&self, container: Ctx, parent: Ctx) -> Option<SymbolId> {
        let Ctx::Sym(c) = container else { return None };
        if let Some(t) = sym::special_reference(self.ftype, c) {
            return Some(t);
        }
        if let Ctx::Sym(p) = parent {
            // SPECIAL_PARENT_FRAGMENT_REFERENCES: $538 → {"yj.print.style": False}
            if self.ftype == sym::DOCUMENT_DATA && self.kdf.symtab().name(p) == "yj.print.style" {
                return None;
            }
            if let Some(t) = sym::nested_reference(p, c) {
                return Some(t);
            }
        }
        sym::common_reference(c)
    }

    fn eid(&mut self, eid: Eid, container: Ctx) {
        let Ctx::Sym(c) = container else { return };
        if (c == 155 || c == sym::KFX_ID) && !sym::NON_EID_DEFINING_TYPES.contains(&self.ftype) {
            self.eid_defs.insert(eid);
        } else if sym::EID_REFERENCES.contains(&c) && !(eid == Eid::Int(0) && self.ftype == 265) {
            self.eid_refs.insert(eid);
        }
    }
}

/// kfxlib's `is_kpf_prepub`: true unless the book metadata names an ASIN or similar.
pub fn is_kpf_prepub(kdf: &Kdf) -> Result<bool> {
    let Some(frag) = kdf.first_of(sym::BOOK_METADATA) else {
        return Ok(true);
    };
    let data = kdf.read(frag)?;
    let keys = title_metadata(data.value()?)?;
    Ok(!keys
        .iter()
        .any(|(k, _)| ["ASIN", "asset_id", "cde_content_type", "content_id"].contains(k)))
}

/// The `kindle_title_metadata` key/value pairs of a `$490` book-metadata value.
fn title_metadata(value: Value<'_>) -> Result<Vec<(&str, Value<'_>)>> {
    let mut out = Vec::new();
    let Some(st) = value.as_struct() else { return Ok(out) };
    let Some(categories) = st.get(sym::CATEGORISED_METADATA)?.and_then(|v| v.as_seq()) else {
        return Ok(out);
    };
    for cm in categories {
        let Some(cm) = cm?.as_struct() else { continue };
        if cm.get(sym::CATEGORY)?.and_then(|v| v.as_str()) != Some("kindle_title_metadata") {
            continue;
        }
        if let Some(kvs) = cm.get(sym::METADATA)?.and_then(|v| v.as_seq()) {
            for kv in kvs {
                let Some(kv) = kv?.as_struct() else { continue };
                if let (Some(k), Some(v)) = (kv.get(sym::KEY)?.and_then(|v| v.as_str()), kv.get(sym::VALUE)?) {
                    out.push((k, v));
                }
            }
        }
        break;
    }
    Ok(out)
}

/// kfxlib's `get_metadata_value("cover_image")`.
fn cover_image(kdf: &Kdf) -> Result<Option<String>> {
    if let Some(frag) = kdf.first_of(sym::BOOK_METADATA) {
        let data = kdf.read(frag)?;
        for (k, v) in title_metadata(data.value()?)? {
            if k == "cover_image" {
                return Ok(kdf.ref_name(v).map(Into::into).or(v.as_str().map(Into::into)));
            }
        }
    }
    if let Some(frag) = kdf.first_of(sym::METADATA) {
        let data = kdf.read(frag)?;
        if let Some(st) = data.value()?.as_struct()
            && let Some(v) = st.get(sym::COVER_IMAGE)?
        {
            return Ok(kdf.ref_name(v).map(Into::into).or(v.as_str().map(Into::into)));
        }
    }
    Ok(None)
}

pub fn analyze(kdf: &Kdf, diag: &mut Diagnostics) -> Result<Usage> {
    let kpf_prepub = is_kpf_prepub(kdf)?;
    let mut discovered: BTreeSet<FragmentKey> = BTreeSet::new();
    for f in kdf.fragments() {
        let t = f.ftype();
        let root = sym::ROOT_FRAGMENT_TYPES.contains(&t) && t != 419;
        if root || (kpf_prepub && t == 610) || !sym::is_known_fragment_type(t) {
            discovered.insert(f.key.clone());
        }
    }
    if let Some(cover) = cover_image(kdf)? {
        discovered.insert(FragmentKey::new(164, Some(&cover)));
    }

    let mut visited: HashSet<FragmentKey> = HashSet::new();
    for t in sym::CONTAINER_FRAGMENT_TYPES {
        visited.insert(FragmentKey::new(t, None));
        visited.insert(FragmentKey::new(t, Some(&format!("${t}"))));
    }

    let mut missing = BTreeSet::new();
    let (mut eid_defs, mut eid_refs) = (HashSet::new(), HashSet::new());
    while !discovered.is_empty() {
        let next: Vec<FragmentKey> = discovered.into_iter().filter(|k| !visited.contains(k)).collect();
        discovered = BTreeSet::new();
        for key in next {
            let Some(frag) = kdf.get_key(&key) else {
                missing.insert(key);
                continue;
            };
            let mut w = Walker {
                kdf,
                ftype: frag.ftype(),
                kpf_prepub,
                mandatory: BTreeSet::new(),
                optional: BTreeSet::new(),
                eid_defs: &mut eid_defs,
                eid_refs: &mut eid_refs,
            };
            if !frag.raw {
                let data = kdf.read(frag)?;
                w.walk(data.value()?, Ctx::Sym(frag.ftype()), Ctx::None, 0)?;
            }
            discovered.extend(w.mandatory);
            discovered.extend(w.optional);
            visited.insert(key);
        }
    }

    for key in &missing {
        let name = kdf.symtab().name(key.ftype).into_owned();
        let fid = key.fid.clone();
        if key.ftype == sym::AUXILIARY_DATA {
            diag.warning("kdf.missing_fragment", || {
                format!("referenced fragment is missing: {name} {fid:?}")
            });
        } else {
            diag.error("kdf.missing_fragment", || {
                format!("referenced fragment is missing: {name} {fid:?}")
            });
        }
    }

    let mut usage = Usage {
        missing: missing.into_iter().collect(),
        ..Usage::default()
    };
    for (i, f) in kdf.fragments().iter().enumerate() {
        let t = f.ftype();
        if visited.contains(&f.key) {
            usage.reachable.push(i);
        } else if sym::CONTAINER_FRAGMENT_TYPES.contains(&t) || f.fid() == Some(&format!("${t}")) {
            diag.error("kdf.unexpected_root", || {
                format!("unexpected root fragment {:?}", f.key)
            });
        } else if t == sym::AUXILIARY_DATA && kdf.is_dictionary() {
        } else if !(kpf_prepub && [391, 266, 259, 260, 608].contains(&t)) {
            usage.unreferenced.push(i);
        }
    }
    if !usage.unreferenced.is_empty() {
        let n = usage.unreferenced.len();
        diag.error("kdf.unreferenced", || format!("{n} unreferenced fragments"));
    }
    let mut undefined: Vec<String> = eid_refs
        .difference(&eid_defs)
        .map(|e| match e {
            Eid::Sym(s) if s.is_empty() => "''".to_string(),
            Eid::Sym(s) => s.to_string(),
            Eid::Int(0) => "''".to_string(),
            Eid::Int(i) => i.to_string(),
        })
        .collect();
    undefined.sort();
    usage.undefined_eids = undefined.len();
    if usage.undefined_eids > 0 {
        let n = usage.undefined_eids;
        let sample = undefined[..n.min(10)].join(", ");
        diag.debug("kdf.undefined_eids", || format!("undefined EIDs: {sample} ({n} total)"));
    }
    undefined.truncate(10);
    usage.undefined_eid_sample = undefined;
    Ok(usage)
}
