//! BibLaTeX's data inheritance: `xdata`, `crossref` and `xref`, resolved on the raw entries
//! before they become references, as biber does.
//!
//! - `xdata = {a, b}` copies every field of the `@xdata` entries `a` and `b` that the entry
//!   lacks, the first listed first. `@xdata` entries hold data only and are not records.
//! - `crossref = {p}` copies the fields of `p` that the entry lacks, after `p`'s own
//!   inheritance, under BibLaTeX's default rules: a book's `title` becomes an `@inbook`'s
//!   `booktitle`, a periodical's `title` an article's `journaltitle`, a multi-volume work's
//!   `title` the `maintitle`, and fields that identify an entry (`ids`, `crossref`, `label`,
//!   `shorthand`, ...) are not inherited. Like biber, the parent's fields are taken in
//!   alphabetical order, and a field the entry already has is never replaced.
//! - `xref = {p}` names a parent and inherits nothing.
//!
//! xdata is resolved before crossref, so a parent's xdata reaches its children. A key that
//! names no entry, and a chain that comes back to an entry it started from, are reported; the
//! rest of the chain still applies. Chains are walked without recursion, so their length is
//! limited only by memory.

use std::collections::HashMap;

use super::Entry;
use crate::diag::Diagnostic;

/// Fields BibLaTeX never inherits through `crossref` (`\DeclareDataInheritance{*}{*}`).
const NOT_INHERITED: &[&str] = &[
    "ids",
    "crossref",
    "xref",
    "xdata",
    "entryset",
    "entrysubtype",
    "execute",
    "label",
    "options",
    "presort",
    "related",
    "relatedoptions",
    "relatedstring",
    "relatedtype",
    "shorthand",
    "shorthandintro",
    "sortkey",
];

/// Driver aliases biber applies before inheritance.
fn canonical(kind: &str) -> &str {
    match kind {
        "conference" => "inproceedings",
        "electronic" | "www" => "online",
        "mastersthesis" | "phdthesis" => "thesis",
        "techreport" => "report",
        k => k,
    }
}

/// The fields a parent's field becomes in a child under BibLaTeX's default inheritance rules
/// (`biblatex.def`): none when it is not inherited, its own name when no rule names it.
fn targets<'f>(parent: &str, child: &str, field: &'f str) -> Vec<&'f str> {
    let (parent, child) = (canonical(parent), canonical(child));
    const MAIN: &[(&str, &str)] = &[
        ("title", "maintitle"),
        ("subtitle", "mainsubtitle"),
        ("titleaddon", "maintitleaddon"),
    ];
    const BOOK: &[(&str, &str)] = &[
        ("title", "booktitle"),
        ("subtitle", "booksubtitle"),
        ("titleaddon", "booktitleaddon"),
    ];
    const JOURNAL: &[(&str, &str)] = &[("title", "journaltitle"), ("subtitle", "journalsubtitle")];
    // A book's author is also the part's: `author` and `bookauthor`.
    if field == "author"
        && matches!(parent, "mvbook" | "book")
        && matches!(child, "inbook" | "bookinbook" | "suppbook")
    {
        return vec!["author", "bookauthor"];
    }
    let titles = match (parent, child) {
        ("mvbook", "book" | "inbook" | "bookinbook" | "suppbook")
        | (
            "mvcollection" | "mvreference",
            "collection" | "reference" | "incollection" | "inreference" | "suppcollection",
        )
        | ("mvproceedings", "proceedings" | "inproceedings") => MAIN,
        ("book", "inbook" | "bookinbook" | "suppbook")
        | ("collection" | "reference", "incollection" | "inreference" | "suppcollection")
        | ("proceedings", "inproceedings") => BOOK,
        ("periodical", "article" | "suppperiodical") => JOURNAL,
        _ => &[],
    };
    if let Some((_, to)) = titles.iter().find(|(from, _)| *from == field) {
        return vec![to];
    }
    // The title fields that have no home in the child.
    let unplaced = matches!(
        field,
        "shorttitle" | "sorttitle" | "indextitle" | "indexsorttitle"
    );
    if (!titles.is_empty() && unplaced) || NOT_INHERITED.contains(&field) {
        return Vec::new();
    }
    vec![field]
}

fn field<'e>(e: &'e Entry, name: &str) -> Option<&'e str> {
    e.fields
        .iter()
        .find(|(n, _, _)| n == name)
        .map(|(_, v, _)| v.as_str())
}

fn has(e: &Entry, name: &str) -> bool {
    e.fields.iter().any(|(n, _, _)| n == name)
}

/// A key as written in `crossref`, `xref` or `xdata`: braces and spaces around it do not count.
fn clean_key(k: &str) -> &str {
    k.trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .trim()
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    New,
    Open,
    Done,
}

/// Visit every entry after its parents, without recursion. `parents[i]` lists entry `i`'s
/// parents in order; `apply(child, parent)` runs once the parent is done. A link back to an
/// entry still open closes a cycle: it goes to `cycle` and is not followed.
fn walk(
    parents: &[Vec<usize>],
    mut apply: impl FnMut(usize, usize),
    mut cycle: impl FnMut(usize, usize),
) {
    let mut state = vec![State::New; parents.len()];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for start in 0..parents.len() {
        if state[start] != State::New {
            continue;
        }
        state[start] = State::Open;
        stack.push((start, 0));
        while let Some(top) = stack.len().checked_sub(1) {
            let (node, next) = stack[top];
            let Some(&p) = parents[node].get(next) else {
                state[node] = State::Done;
                stack.pop();
                continue;
            };
            match state[p] {
                State::Done => {
                    apply(node, p);
                    stack[top].1 += 1;
                }
                State::Open => {
                    cycle(node, p);
                    stack[top].1 += 1;
                }
                State::New => {
                    state[p] = State::Open;
                    stack.push((p, 0));
                }
            }
        }
    }
}

/// Links from entries to the entries they name.
struct Links {
    xdata: Vec<Vec<usize>>,
    crossref: Vec<Vec<usize>>,
}

/// Find what every `xdata`, `crossref` and `xref` names, reporting keys that name nothing.
fn links(entries: &[Entry], note: &mut dyn FnMut(Diagnostic, usize)) -> Links {
    // The first entry with a key wins, as in BibTeX.
    let mut by_key: HashMap<&str, usize> = HashMap::new();
    let mut by_folded: HashMap<String, usize> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        if !e.key.is_empty() {
            by_key.entry(e.key.as_str()).or_insert(i);
            by_folded.entry(e.key.to_lowercase()).or_insert(i);
        }
    }
    // A key that matches only when case is ignored is used, with a warning.
    let find = |note: &mut dyn FnMut(Diagnostic, usize), i: usize, how: &str, key: &str| {
        let e = &entries[i];
        if let Some(&j) = by_key.get(key) {
            return Some(j);
        }
        if let Some(&j) = by_folded.get(&key.to_lowercase()) {
            note(
                Diagnostic::warning(
                    "bibtex.parent-case",
                    format!(
                        "Entry {:?} names {how} {key:?}, which is written {:?}; used it",
                        e.key, entries[j].key
                    ),
                ),
                i,
            );
            return Some(j);
        }
        note(
            Diagnostic::warning(
                "bibtex.missing-parent",
                format!(
                    "Entry {:?} names {how} {key:?}, but no entry has that key; nothing inherited from it",
                    e.key
                ),
            ),
            i,
        );
        None
    };
    let mut out = Links {
        xdata: vec![Vec::new(); entries.len()],
        crossref: vec![Vec::new(); entries.len()],
    };
    for (i, e) in entries.iter().enumerate() {
        if let Some(list) = field(e, "xdata") {
            for key in list.split(',').map(clean_key).filter(|k| !k.is_empty()) {
                let Some(j) = find(note, i, "xdata", key) else {
                    continue;
                };
                if entries[j].kind != "xdata" {
                    note(
                        Diagnostic::warning(
                            "bibtex.xdata-not-xdata",
                            format!(
                                "Entry {:?} takes xdata from {:?}, an @{} entry; BibLaTeX reads xdata only from @xdata entries, so Pinakes does too",
                                e.key, entries[j].key, entries[j].kind
                            ),
                        ),
                        i,
                    );
                    continue;
                }
                out.xdata[i].push(j);
            }
        }
        if let Some(key) = field(e, "crossref")
            .map(clean_key)
            .filter(|k| !k.is_empty())
        {
            if let Some(j) = find(note, i, "crossref", key) {
                out.crossref[i].push(j);
            }
        }
        if let Some(key) = field(e, "xref").map(clean_key).filter(|k| !k.is_empty()) {
            find(note, i, "xref", key);
        }
    }
    out
}

/// Resolve xdata and crossref and check xref, then drop the `@xdata` entries. Diagnostics name
/// each record by its place among the entries that remain.
pub(super) fn resolve(entries: &mut Vec<Entry>, diags: &mut Vec<Diagnostic>) {
    let wanted = entries
        .iter()
        .any(|e| e.kind == "xdata" || has(e, "crossref") || has(e, "xref") || has(e, "xdata"));
    if !wanted {
        return;
    }
    // Each entry's place among the records, and its line, for diagnostics.
    let mut place = Vec::with_capacity(entries.len());
    let mut n = 0;
    for e in entries.iter() {
        place.push((e.kind != "xdata").then_some(n));
        n += usize::from(e.kind != "xdata");
    }
    let lines: Vec<usize> = entries.iter().map(|e| e.line).collect();
    let mut note = |d: Diagnostic, i: usize| {
        let d = d.at(lines[i]);
        diags.push(match place[i] {
            Some(r) => d.record(r),
            None => d,
        });
    };
    let links = links(entries, &mut note);
    let mut cycles: Vec<(usize, usize, &str)> = Vec::new();
    walk(
        &links.xdata,
        |child, parent| {
            let from: Vec<_> = entries[parent]
                .fields
                .iter()
                .filter(|(n, _, _)| n != "xdata")
                .cloned()
                .collect();
            let child = &mut entries[child];
            for f in from {
                if !has(child, &f.0) {
                    child.fields.push(f);
                }
            }
        },
        |c, p| cycles.push((c, p, "xdata")),
    );
    walk(
        &links.crossref,
        |child, parent| {
            let mut from = entries[parent].fields.clone();
            from.sort_by(|a, b| a.0.cmp(&b.0));
            let (pk, ck) = (entries[parent].kind.clone(), entries[child].kind.clone());
            let child = &mut entries[child];
            for (name, value, line) in from {
                for to in targets(&pk, &ck, &name) {
                    if !has(child, to) {
                        child.fields.push((to.to_string(), value.clone(), line));
                    }
                }
            }
        },
        |c, p| cycles.push((c, p, "crossref")),
    );
    for (c, p, how) in cycles {
        note(
            Diagnostic::error(
                "bibtex.inheritance-cycle",
                format!(
                    "Entry {:?} takes {how} from {:?}, which leads back to it; that link is not followed",
                    entries[c].key, entries[p].key
                ),
            ),
            c,
        );
    }
    entries.retain(|e| e.kind != "xdata");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: &str, key: &str, fields: &[(&str, &str)]) -> Entry {
        Entry {
            kind: kind.into(),
            key: key.into(),
            line: 1,
            fields: fields
                .iter()
                .map(|(n, v)| (n.to_string(), v.to_string(), 1))
                .collect(),
        }
    }

    #[test]
    fn rules() {
        assert_eq!(
            targets("proceedings", "inproceedings", "title"),
            ["booktitle"]
        );
        assert_eq!(
            targets("book", "inbook", "author"),
            ["author", "bookauthor"]
        );
        assert_eq!(targets("mvbook", "book", "title"), ["maintitle"]);
        assert_eq!(targets("periodical", "article", "title"), ["journaltitle"]);
        assert!(targets("book", "inbook", "shorttitle").is_empty());
        assert!(targets("book", "inbook", "ids").is_empty());
        assert_eq!(targets("book", "misc", "title"), ["title"]);
        assert_eq!(targets("book", "misc", "shorttitle"), ["shorttitle"]);
        assert_eq!(targets("proceedings", "conference", "title"), ["booktitle"]);
    }

    #[test]
    fn deep_chains_do_not_recurse() {
        // A chain of 200,000 crossrefs, and a ring of the same length.
        let n = 200_000;
        let mut chain: Vec<Entry> = (0..n)
            .map(|i| {
                if i == n - 1 {
                    entry("book", &i.to_string(), &[("year", "2020")])
                } else {
                    entry(
                        "book",
                        &i.to_string(),
                        &[("crossref", &(i + 1).to_string())],
                    )
                }
            })
            .collect();
        let mut diags = Vec::new();
        resolve(&mut chain, &mut diags);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(field(&chain[0], "year"), Some("2020"));

        let mut ring: Vec<Entry> = (0..n)
            .map(|i| {
                entry(
                    "book",
                    &i.to_string(),
                    &[("crossref", &((i + 1) % n).to_string())],
                )
            })
            .collect();
        let mut diags = Vec::new();
        resolve(&mut ring, &mut diags);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, "bibtex.inheritance-cycle");
    }
}
