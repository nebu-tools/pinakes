//! What a writer could not write as it was.
//!
//! A writer pushes a diagnostic for each loss it knows of by name (`ris.et-al`,
//! `bibtex.verbatim-encoded`). Then, for every format Pinakes also reads, [`read_back`] reads the
//! output again and compares each record with the one written, member by member in CSL-JSON
//! terms. Each member that came back different, or did not come back, and that no diagnostic
//! names yet, gets one. So a loss nobody thought of is still reported.
//!
//! The comparison is between what the reader makes of the record and what it makes of the
//! output: the record is normalised first, as every reader normalises what it returns. Three
//! things are not compared. `id` is the record's place in its source, which no writer promises
//! to keep. A citation key the record did not have is one the writer made up, not one it
//! changed. And a blank value (empty or white space, or a list of them), here or inside a name or a
//! date, is the same as none. RIS keeps a citation key on its `ID` line, which reads back as the
//! `id`; that counts as kept.

use crate::diag::Diagnostic;
use crate::formats::{csl, Format};
use crate::model::Reference;
use serde_json::{Map, Value};

/// The record as CSL-JSON members, without the forms the CSL-JSON reader kept beside the typed
/// values, and with `custom` members flattened to `custom.<name>`.
pub(crate) fn members(r: &Reference) -> Map<String, Value> {
    let Value::Object(mut m) = csl::to_value(&csl::without_forms(r)) else {
        return Map::new();
    };
    m.remove("id");
    if let Some(Value::Object(custom)) = m.remove("custom") {
        for (k, v) in custom {
            m.insert(format!("custom.{k}"), v);
        }
    }
    m
}

/// Whether a value says nothing: empty or white space, or a list or object of such values.
fn blank(v: &Value) -> bool {
    match v {
        Value::String(s) => s.trim().is_empty(),
        Value::Array(a) => a.iter().all(blank),
        Value::Object(o) => o.values().all(blank),
        _ => false,
    }
}

/// Remove the members of every object inside `v` that say nothing.
fn drop_blank(v: &mut Value) {
    match v {
        Value::Array(a) => a.iter_mut().for_each(drop_blank),
        Value::Object(o) => {
            o.retain(|_, v| !blank(v));
            o.values_mut().for_each(drop_blank);
        }
        _ => {}
    }
}

/// RIS writes the citation key on its `ID` line, which its reader takes as the record's `id`.
/// The key is then in the output, so count it as read back.
fn held_as_id(
    format: Format,
    back: &Reference,
    want: &Map<String, Value>,
    got: &mut Map<String, Value>,
) {
    if format != Format::Ris || got.contains_key("citation-key") {
        return;
    }
    if let Some(Value::String(k)) = want.get("citation-key") {
        if *k == back.id {
            got.insert("citation-key".into(), Value::String(k.clone()));
        }
    }
}

/// How diagnostics name a record: its position, and its key or identifier.
pub(crate) fn label(r: &Reference, index: usize) -> String {
    let name = r.citation_key.as_deref().unwrap_or(&r.id);
    if name.is_empty() || name == (index + 1).to_string() {
        format!("Record {}", index + 1)
    } else {
        format!("Record {} ({name})", index + 1)
    }
}

/// A value for a message: compact JSON, cut short.
pub(crate) fn show(v: &Value) -> String {
    let s = match v {
        Value::String(s) => format!("{s:?}"),
        other => other.to_string(),
    };
    if s.chars().count() > 60 {
        let cut: String = s.chars().take(57).collect();
        format!("{cut}...")
    } else {
        s
    }
}

fn codes(format: Format) -> (&'static str, &'static str, &'static str) {
    match format {
        Format::Ris => ("ris.left-out", "ris.changed", "ris.records"),
        Format::Bibtex | Format::Biblatex => {
            ("bibtex.left-out", "bibtex.changed", "bibtex.records")
        }
        _ => ("csl.left-out", "csl.changed", "csl.records"),
    }
}

/// Read `text`, which `format`'s writer made from `refs`, and add a diagnostic for each member
/// that does not come back as written and that `diags` does not already name.
pub(crate) fn read_back(
    format: Format,
    refs: &[Reference],
    text: &str,
    diags: &mut Vec<Diagnostic>,
) {
    let (left_out, changed, records) = codes(format);
    let back = crate::formats::read_text(format, text, &mut Vec::new());
    if back.len() != refs.len() {
        diags.push(Diagnostic::error(
            records,
            format!(
                "Wrote {} records but the output reads back as {}",
                refs.len(),
                back.len()
            ),
        ));
        return;
    }
    for (i, (r, b)) in refs.iter().zip(&back).enumerate() {
        let mut want = r.clone();
        crate::normalise::normalise(&mut want);
        let mut want_members = members(&want);
        let mut got_members = members(b);
        for m in [&mut want_members, &mut got_members] {
            m.retain(|_, v| !blank(v));
            m.values_mut().for_each(drop_blank);
        }
        held_as_id(format, b, &want_members, &mut got_members);
        let named = |field: &str, diags: &[Diagnostic]| {
            diags
                .iter()
                .any(|d| d.record == Some(i) && d.field.as_deref() == Some(field))
        };
        let label = label(r, i);
        for (k, w) in &want_members {
            match got_members.get(k) {
                Some(g) if g == w => {}
                _ if named(k, diags) => {}
                Some(g) => diags.push(
                    Diagnostic::warning(
                        changed,
                        format!(
                            "{label}: {format} cannot hold {k} as it is; wrote {}, which reads back as {}",
                            show(w),
                            show(g)
                        ),
                    )
                    .record(i)
                    .field(k.as_str()),
                ),
                None => diags.push(
                    Diagnostic::warning(
                        left_out,
                        format!("{label}: {format} has no place for {k}; left out {}", show(w)),
                    )
                    .record(i)
                    .field(k.as_str()),
                ),
            }
        }
        for (k, g) in &got_members {
            if want_members.contains_key(k)
                || (k == "citation-key" && want.citation_key.is_none())
                || named(k, diags)
            {
                continue;
            }
            diags.push(
                Diagnostic::warning(
                    changed,
                    format!(
                        "{label}: the output reads back with {k} {}, which the record did not have",
                        show(g)
                    ),
                )
                .record(i)
                .field(k.as_str()),
            );
        }
    }
    diags.sort_by_key(|d| d.record);
}
