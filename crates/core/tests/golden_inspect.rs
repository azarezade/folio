//! Phase 1 golden test: `inspect` must agree with the kfxlib reference harness
//! (`tools/reference/dump_notebook.py`) on fragment counts and types, sections
//! and pages, for every dumped notebook in `fixtures/reference/`.
//!
//! The fixtures are made from your own notebooks and are not in git; without them
//! this test passes with a note.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn native_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn summaries() -> Vec<Value> {
    let dir = native_root().join("fixtures/reference");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<Value> = entries
        .filter_map(|e| std::fs::read_to_string(e.ok()?.path().join("summary.json")).ok())
        .map(|s| serde_json::from_str(&s).unwrap())
        .collect();
    out.sort_by_key(|s| s["notebook"].as_str().unwrap().to_string());
    out
}

/// The notebook a summary was made from; `source` is relative to the repo root.
fn source(summary: &Value) -> PathBuf {
    let src = Path::new(summary["source"].as_str().unwrap());
    if src.is_absolute() {
        src.to_path_buf()
    } else {
        native_root().join("..").join(src)
    }
}

/// kfxlib's "Undefined EIDs: a, b, … (N total)" message, from the harness's grouped log:
/// (total, first names).
fn harness_undefined_eids(summary: &Value) -> (usize, Vec<String>) {
    let Some(msg) = summary["kfxlib_log"]["errors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["example"].as_str())
        .find(|m| m.starts_with("Undefined EIDs: "))
    else {
        return (0, Vec::new());
    };
    let list = &msg["Undefined EIDs: ".len()..];
    let total = list
        .rsplit_once(" total)")
        .and_then(|(head, _)| head.rsplit('(').next()?.parse().ok());
    // The harness shortens long messages with " … "; names before it are complete.
    let head = list.split(" … ").next().unwrap();
    let mut names: Vec<String> = head.split(", ").map(Into::into).collect();
    if list.contains(" … ") || names.last().is_some_and(|n| n.starts_with("...")) {
        names.pop();
    }
    let total = total.unwrap_or(names.len());
    (total, names)
}

fn check(summary: &Value) -> Vec<String> {
    let name = &summary["notebook"].as_str().unwrap()[..8.min(summary["notebook"].as_str().unwrap().len())];
    let report = match folio_core::inspect(&source(summary), None, false) {
        Ok(r) => r,
        Err(e) => return vec![format!("{name}: inspect failed: {e}")],
    };
    let mut problems = Vec::new();
    let mut expect = |what: &str, ours: String, theirs: String| {
        if ours != theirs {
            problems.push(format!("{name}: {what}: ours {ours}, harness {theirs}"));
        }
    };

    let by_type: BTreeMap<String, usize> = summary["fragments"]["by_type"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_u64().unwrap() as usize))
        .collect();
    expect(
        "fragment total",
        report.reachable.total.to_string(),
        summary["fragments"]["total"].to_string(),
    );
    expect(
        "fragment types",
        format!("{:?}", report.reachable.by_type),
        format!("{by_type:?}"),
    );

    expect(
        "page count",
        report.page_count.to_string(),
        summary["page_count"].to_string(),
    );
    let pages: Vec<String> = summary["page_sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().into())
        .collect();
    if report.page_sections != pages {
        expect("page order", "differs".into(), "".into());
    }
    expect(
        "sections",
        report.sections_processed.to_string(),
        summary["sections"]["processed"].to_string(),
    );
    let kinds: BTreeMap<String, usize> = summary["sections"]["by_kind"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_u64().unwrap() as usize))
        .collect();
    expect(
        "section kinds",
        format!("{:?}", report.sections_by_kind),
        format!("{kinds:?}"),
    );
    let canvas: BTreeMap<String, usize> = summary["canvas_sizes"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_u64().unwrap() as usize))
        .collect();
    expect(
        "canvas sizes",
        format!("{:?}", report.canvas_sizes),
        format!("{canvas:?}"),
    );

    let templates: Vec<(String, String)> = report
        .templates
        .iter()
        .map(|t| (t.section.clone(), t.template_type.clone().unwrap_or_default()))
        .collect();
    let theirs: Vec<(String, String)> = summary["templates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["section"].as_str().unwrap().into(),
                t["template_type"].as_str().unwrap().into(),
            )
        })
        .collect();
    expect("templates", format!("{templates:?}"), format!("{theirs:?}"));

    let ros: Vec<(Option<String>, Vec<String>)> = report
        .reading_orders
        .iter()
        .map(|r| (r.name.clone(), r.sections.clone()))
        .collect();
    let theirs: Vec<(Option<String>, Vec<String>)> = summary["reading_orders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["name"].as_str().map(Into::into),
                r["sections"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| s.as_str().unwrap().into())
                    .collect(),
            )
        })
        .collect();
    if ros != theirs {
        expect(
            "reading orders",
            format!("{} orders", ros.len()),
            format!("{} orders (or contents differ)", theirs.len()),
        );
    }

    let (eids, eid_names) = harness_undefined_eids(summary);
    expect("undefined EIDs", report.undefined_eids.to_string(), eids.to_string());
    if !report.undefined_eid_sample.starts_with(&eid_names) {
        expect(
            "undefined EID names",
            format!("{:?}", report.undefined_eid_sample),
            format!("{eid_names:?}"),
        );
    }
    expect("missing references", report.missing_references.to_string(), "0".into());
    problems
}

#[test]
fn inspect_matches_reference_harness() {
    let summaries = summaries();
    if summaries.is_empty() {
        eprintln!("no fixtures/reference dumps; skipping (see tools/reference/README.md)");
        return;
    }
    let problems: Vec<String> = std::thread::scope(|s| {
        let handles: Vec<_> = summaries.iter().map(|sum| s.spawn(move || check(sum))).collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    assert!(
        problems.is_empty(),
        "{} mismatches:\n{}",
        problems.len(),
        problems.join("\n")
    );
    eprintln!("{} notebooks match the reference harness", summaries.len());
}
