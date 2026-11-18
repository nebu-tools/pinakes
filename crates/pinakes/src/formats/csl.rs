//! CSL-JSON: the model's own serialisation, and a reader and writer for CSL-JSON files.
//!
//! Fields CSL defines map to typed slots. Pinakes' own additions go under `custom`, which CSL-JSON
//! 1.0.2 reserves for this: `arxiv`, `accession-number`, `database`, `publication-type` and
//! `source-fields`. Any other field, at the top level or inside `custom`, is kept and written back.

use crate::dates::parse_date;
use crate::diag::Diagnostic;
use crate::model::{Date, Name, Reference, SourceField, Ymd};
use serde_json::{json, Map, Value};

/// The reference as a CSL-JSON object.
pub fn to_value(r: &Reference) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), Value::String(r.id.clone()));
    m.insert(
        "type".into(),
        Value::String(if r.kind.is_empty() {
            "document".into()
        } else {
            r.kind.clone()
        }),
    );
    let mut put = |k: &str, v: &Option<String>| {
        if let Some(v) = v {
            m.insert(k.into(), Value::String(v.clone()));
        }
    };
    put("citation-key", &r.citation_key);
    put("title", &r.title);
    put("title-short", &r.title_short);
    put("container-title", &r.container_title);
    put("container-title-short", &r.container_title_short);
    put("collection-title", &r.collection_title);
    put("publisher", &r.publisher);
    put("publisher-place", &r.publisher_place);
    put("volume", &r.volume);
    put("issue", &r.issue);
    put("number", &r.number);
    put("page", &r.page);
    put("edition", &r.edition);
    put("genre", &r.genre);
    put("abstract", &r.abstract_text);
    put("language", &r.language);
    put("note", &r.note);
    put("URL", &r.url);
    put("DOI", &r.doi);
    put("PMID", &r.pmid);
    put("PMCID", &r.pmcid);
    put("ISBN", &r.isbn);
    put("ISSN", &r.issn);
    if !r.keywords.is_empty() {
        m.insert("keyword".into(), Value::String(r.keywords.join(", ")));
    }
    if !r.author.is_empty() {
        m.insert(
            "author".into(),
            Value::Array(r.author.iter().map(name_value).collect()),
        );
    }
    if !r.editor.is_empty() {
        m.insert(
            "editor".into(),
            Value::Array(r.editor.iter().map(name_value).collect()),
        );
    }
    if let Some(d) = &r.issued {
        m.insert("issued".into(), date_value(d));
    }
    if let Some(d) = &r.accessed {
        m.insert("accessed".into(), date_value(d));
    }
    let kept_custom = match r.extra.get("custom") {
        Some(Value::Object(c)) => c.clone(),
        _ => Map::new(),
    };
    let mut custom = Map::new();
    let mut put = |k: &str, v: &Option<String>| {
        if let Some(v) = v {
            custom.insert(k.into(), Value::String(v.clone()));
        }
    };
    put("arxiv", &r.arxiv);
    put("accession-number", &r.accession);
    put("database", &r.database);
    if !r.publication_types.is_empty() {
        custom.insert("publication-type".into(), json!(r.publication_types));
    }
    if !r.source_fields.is_empty() {
        custom.insert(
            "source-fields".into(),
            Value::Array(
                r.source_fields
                    .iter()
                    .map(|f| json!({"format": f.format, "name": f.name, "value": f.value}))
                    .collect(),
            ),
        );
    }
    if let Some(raw) = kept_custom.get("publication-type") {
        if publication_types_of(raw) == r.publication_types {
            custom.insert("publication-type".into(), raw.clone());
        }
    }
    if let Some(raw) = kept_custom.get("source-fields") {
        if source_fields_of(raw) == r.source_fields {
            custom.insert("source-fields".into(), raw.clone());
        }
    }
    merge_raw(
        &mut custom,
        &kept_custom,
        CUSTOM_TYPED,
        &["publication-type", "source-fields"],
    );
    if let Some(raw) = r.extra.get("keyword") {
        if keyword_list(raw) == r.keywords {
            m.insert("keyword".into(), raw.clone());
        }
    }
    for role in ["author", "editor"] {
        if let Some(raw) = r.extra.get(role) {
            let names = if role == "author" {
                &r.author
            } else {
                &r.editor
            };
            if &names_of(raw) == names {
                m.insert(role.into(), raw.clone());
            }
        }
    }
    merge_raw(
        &mut m,
        &r.extra,
        TYPED_TOP,
        &["custom", "keyword", "author", "editor"],
    );
    if !custom.is_empty() {
        m.insert("custom".into(), Value::Object(custom));
    }
    Value::Object(m)
}

fn name_value(n: &Name) -> Value {
    let mut m = Map::new();
    let mut put = |k: &str, v: &Option<String>| {
        if let Some(v) = v {
            m.insert(k.into(), Value::String(v.clone()));
        }
    };
    put("family", &n.family);
    put("given", &n.given);
    put("non-dropping-particle", &n.particle);
    put("dropping-particle", &n.dropping_particle);
    put("suffix", &n.suffix);
    put("literal", &n.literal);
    merge_raw(&mut m, &n.extra, NAME_PARTS, &[]);
    Value::Object(m)
}

/// Put back the members a reader kept as they came. A kept member replaces the typed one when it
/// still reads as the same text (`12` for "12"). A member the model types (`typed`) but no
/// longer holds is written back only if it never read as text: otherwise the model cleared it.
/// Members named in `skip` are left to the caller.
fn merge_raw(m: &mut Map<String, Value>, kept: &Map<String, Value>, typed: &[&str], skip: &[&str]) {
    for (k, raw) in kept {
        if skip.contains(&k.as_str()) {
            continue;
        }
        match m.get(k) {
            Some(Value::String(typed)) => {
                if string_of(raw).as_deref() == Some(typed.as_str()) {
                    m.insert(k.clone(), raw.clone());
                }
            }
            Some(_) => {}
            None => {
                let cleared = typed.contains(&k.as_str())
                    && matches!(raw, Value::String(_) | Value::Number(_))
                    && string_of(raw).is_some();
                if !cleared {
                    m.insert(k.clone(), raw.clone());
                }
            }
        }
    }
}

/// Keep `raw` under `key` when the typed slot will not write it back as it came.
fn keep_raw(kept: &mut Map<String, Value>, key: &str, raw: &Value) {
    if !matches!(raw, Value::String(s) if string_of(raw).as_deref() == Some(s.as_str())) {
        kept.insert(key.to_string(), raw.clone());
    }
}

/// Names as the reader leaves them.
fn names_of(v: &Value) -> Vec<Name> {
    let mut r = Reference {
        author: match v {
            Value::Array(a) => a.iter().filter_map(parse_name_value).collect(),
            other => parse_name_value(other).into_iter().collect(),
        },
        ..Reference::default()
    };
    crate::normalise::normalise(&mut r);
    r.author
}

/// Keywords as the model holds them: single terms, split on commas and semicolons, each once.
fn keyword_list(v: &Value) -> Vec<String> {
    let mut r = Reference {
        keywords: match v {
            Value::Array(a) => a.iter().filter_map(string_of).collect(),
            Value::String(s) => vec![s.clone()],
            _ => Vec::new(),
        },
        ..Reference::default()
    };
    crate::normalise::normalise(&mut r);
    r.keywords
}

fn circa_of(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(_)) => true,
        Some(Value::String(s)) => !s.is_empty(),
        _ => false,
    }
}

fn date_parts_of(v: Option<&Value>) -> (Option<Ymd>, Option<Ymd>) {
    match v {
        Some(Value::Array(parts)) => (
            parts.first().and_then(parse_ymd),
            parts.get(1).and_then(parse_ymd),
        ),
        _ => (None, None),
    }
}

fn ymd_value(d: &Ymd) -> Value {
    let mut parts = vec![json!(d.year)];
    if let Some(m) = d.month {
        parts.push(json!(m));
        if let Some(day) = d.day {
            parts.push(json!(day));
        }
    }
    Value::Array(parts)
}

fn date_value(d: &Date) -> Value {
    let mut m = Map::new();
    if let Some(start) = &d.start {
        let mut parts = vec![ymd_value(start)];
        if let Some(end) = &d.end {
            parts.push(ymd_value(end));
        }
        m.insert("date-parts".into(), Value::Array(parts));
    }
    if d.circa {
        m.insert("circa".into(), Value::Bool(true));
    }
    if let Some(l) = &d.literal {
        m.insert("literal".into(), Value::String(l.clone()));
    }
    if let Some(r) = &d.raw {
        m.insert("raw".into(), Value::String(r.clone()));
    }
    if let Some(raw) = d.extra.get("date-parts") {
        if date_parts_of(Some(raw)) == (d.start, d.end) {
            m.insert("date-parts".into(), raw.clone());
        }
    }
    if let Some(raw) = d.extra.get("circa") {
        if circa_of(Some(raw)) == d.circa {
            m.insert("circa".into(), raw.clone());
        }
    }
    merge_raw(
        &mut m,
        &d.extra,
        &["literal", "raw"],
        &["date-parts", "circa"],
    );
    Value::Object(m)
}

/// A CSL value as a string: CSL allows numbers for volume, issue, page and others.
fn string_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => crate::text::non_empty(s),
        Value::Number(n) => Some(n.to_string()),
        Value::Array(a) => {
            let parts: Vec<String> = a.iter().filter_map(string_of).collect();
            (!parts.is_empty()).then(|| parts.join(", "))
        }
        _ => None,
    }
}

fn parse_name_value(v: &Value) -> Option<Name> {
    match v {
        Value::String(s) => Some(crate::names::parse_name(s)),
        Value::Object(o) => {
            let get = |k: &str| o.get(k).and_then(string_of);
            let mut n = Name {
                family: get("family"),
                given: get("given"),
                particle: get("non-dropping-particle"),
                dropping_particle: get("dropping-particle"),
                suffix: get("suffix"),
                literal: get("literal"),
                extra: Map::new(),
            };
            for (k, raw) in o {
                if NAME_PARTS.contains(&k.as_str()) {
                    keep_raw(&mut n.extra, k, raw);
                } else {
                    n.extra.insert(k.clone(), raw.clone());
                }
            }
            (!n.is_empty() || !n.extra.is_empty()).then_some(n)
        }
        _ => None,
    }
}

fn part(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn parse_ymd(v: &Value) -> Option<Ymd> {
    let a = v.as_array()?;
    let year = part(a.first()?)? as i32;
    let month = a
        .get(1)
        .and_then(part)
        .filter(|m| (1..=12).contains(m))
        .map(|m| m as u8);
    let day = a
        .get(2)
        .and_then(part)
        .filter(|d| (1..=31).contains(d))
        .map(|d| d as u8);
    Some(Ymd {
        year,
        month,
        day: month.and(day),
    })
}

pub(crate) fn parse_date_value(v: &Value) -> Option<Date> {
    match v {
        Value::String(s) => parse_date(s),
        Value::Number(n) => n.as_i64().map(|y| Date::year(y as i32)),
        Value::Object(o) => {
            let mut d = Date::default();
            (d.start, d.end) = date_parts_of(o.get("date-parts"));
            d.circa = circa_of(o.get("circa"));
            d.literal = o.get("literal").and_then(string_of);
            d.raw = o.get("raw").and_then(string_of);
            for (k, raw) in o {
                match k.as_str() {
                    "date-parts" => {
                        let mut canonical = Map::new();
                        if let Some(start) = &d.start {
                            let mut parts = vec![ymd_value(start)];
                            if let Some(end) = &d.end {
                                parts.push(ymd_value(end));
                            }
                            canonical.insert(k.clone(), Value::Array(parts));
                        }
                        if canonical.get(k) != Some(raw) {
                            d.extra.insert(k.clone(), raw.clone());
                        }
                    }
                    "circa" => {
                        if raw != &Value::Bool(true) {
                            d.extra.insert(k.clone(), raw.clone());
                        }
                    }
                    "literal" | "raw" => keep_raw(&mut d.extra, k, raw),
                    _ => {
                        d.extra.insert(k.clone(), raw.clone());
                    }
                }
            }
            if d.start.is_none() {
                // A date with no parts may still say its year in its text.
                if let Some(parsed) = d
                    .raw
                    .as_deref()
                    .or(d.literal.as_deref())
                    .and_then(parse_date)
                {
                    d.start = parsed.start;
                    d.end = parsed.end;
                }
            }
            (d != Date::default()).then_some(d)
        }
        _ => None,
    }
}

/// `custom` members read as text into a typed slot.
const CUSTOM_TYPED: &[&str] = &["arxiv", "accession-number", "database"];

/// Publication types as the reader leaves them.
fn publication_types_of(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a.iter().filter_map(string_of).collect(),
        other => string_of(other).into_iter().collect(),
    }
}

/// Source fields as the reader leaves them: each object element, with its three members as
/// text. A string member is kept exactly, white space included, as the source had it.
fn source_fields_of(v: &Value) -> Vec<SourceField> {
    let Value::Array(a) = v else {
        return Vec::new();
    };
    a.iter()
        .filter(|f| f.is_object())
        .map(|f| {
            let get = |key: &str| match f.get(key) {
                Some(Value::String(s)) => s.clone(),
                other => other.and_then(string_of).unwrap_or_default(),
            };
            SourceField {
                format: get("format"),
                name: get("name"),
                value: get("value"),
            }
        })
        .collect()
}

/// Whether `source-fields` is exactly what the writer makes of the list it reads as.
fn plain_source_fields(v: &Value) -> bool {
    let Value::Array(a) = v else {
        return false;
    };
    a.iter().all(|f| {
        f.as_object().is_some_and(|o| {
            o.len() == 3
                && ["format", "name", "value"]
                    .iter()
                    .all(|k| o.get(*k).is_some_and(Value::is_string))
        })
    })
}

/// Whether the members of `v` are only text, numbers and lists of them: forms the typed slot
/// reads in full.
fn only_text(v: &Value) -> bool {
    match v {
        Value::String(_) | Value::Number(_) => true,
        Value::Array(a) => a
            .iter()
            .all(|x| matches!(x, Value::String(_) | Value::Number(_))),
        _ => false,
    }
}

/// Whether `date-parts` says no more than `start` and `end`: the same numbers, some perhaps
/// written as text.
fn parts_form_only(raw: &Value, start: Option<Ymd>, end: Option<Ymd>) -> bool {
    let Some(start) = start else {
        return false;
    };
    let mut want = vec![ymd_value(&start)];
    if let Some(end) = &end {
        want.push(ymd_value(end));
    }
    let got: Option<Vec<Value>> = raw.as_array().and_then(|ps| {
        ps.iter()
            .map(|p| {
                p.as_array()
                    .and_then(|a| a.iter().map(|x| part(x).map(|n| json!(n))).collect())
                    .map(Value::Array)
            })
            .collect()
    });
    got == Some(want)
}

/// The record without the members the reader kept only for their form (`12` for "12", a
/// name as a string). What is left are the values: a member kept because it holds more than
/// its typed slot (a name list with a number in it, a source field with a member of its own)
/// stays.
pub(crate) fn without_forms(r: &Reference) -> Reference {
    let mut out = r.clone();
    let plain = to_value(&Reference {
        extra: Map::new(),
        ..r.clone()
    });
    let same_text = |raw: &Value, typed: Option<&str>| {
        matches!(raw, Value::String(_) | Value::Number(_))
            && typed.is_some()
            && string_of(raw).as_deref() == typed
    };
    out.extra.retain(|k, raw| match (k.as_str(), &*raw) {
        ("id", _) => false,
        ("keyword", raw) => !(only_text(raw) && keyword_list(raw) == r.keywords),
        ("author" | "editor", raw) => {
            let names = if k == "author" { &r.author } else { &r.editor };
            let parts_are_names = match raw {
                Value::Array(a) => a.iter().all(|n| n.is_string() || n.is_object()),
                other => other.is_string() || other.is_object(),
            };
            !(parts_are_names && &names_of(raw) == names)
        }
        ("custom", _) => true,
        (k, raw) if TYPED_TOP.contains(&k) => {
            let canonical = match k {
                "citation_key" => "citation-key",
                "shortTitle" => "title-short",
                "journalAbbreviation" => "container-title-short",
                "url" => "URL",
                "doi" => "DOI",
                "pmid" => "PMID",
                "pmcid" => "PMCID",
                "isbn" => "ISBN",
                "issn" => "ISSN",
                k => k,
            };
            !same_text(raw, plain.get(canonical).and_then(Value::as_str))
        }
        _ => true,
    });
    if let Some(Value::Object(c)) = out.extra.get_mut("custom") {
        c.retain(|k, raw| match k.as_str() {
            "arxiv" => !same_text(raw, r.arxiv.as_deref()),
            "accession-number" => !same_text(raw, r.accession.as_deref()),
            "database" => !same_text(raw, r.database.as_deref()),
            "publication-type" => {
                !(only_text(raw) && publication_types_of(raw) == r.publication_types)
            }
            "source-fields" => {
                let text_only = matches!(raw, Value::Array(a) if a.iter().all(|f| {
                    f.as_object().is_some_and(|o| {
                        o.iter().all(|(k, v)| {
                            ["format", "name", "value"].contains(&k.as_str())
                                && matches!(v, Value::String(_) | Value::Number(_))
                        })
                    })
                }));
                !(text_only && source_fields_of(raw) == r.source_fields)
            }
            _ => true,
        });
        if c.is_empty() {
            out.extra.remove("custom");
        }
    }
    for n in out.author.iter_mut().chain(out.editor.iter_mut()) {
        let parts = [
            ("family", n.family.clone()),
            ("given", n.given.clone()),
            ("non-dropping-particle", n.particle.clone()),
            ("dropping-particle", n.dropping_particle.clone()),
            ("suffix", n.suffix.clone()),
            ("literal", n.literal.clone()),
        ];
        n.extra.retain(|k, raw| {
            !parts
                .iter()
                .any(|(p, v)| p == k && same_text(raw, v.as_deref()))
        });
    }
    for d in [&mut out.issued, &mut out.accessed].into_iter().flatten() {
        let (start, end, circa) = (d.start, d.end, d.circa);
        let (literal, raw_text) = (d.literal.clone(), d.raw.clone());
        d.extra.retain(|k, raw| match k.as_str() {
            "date-parts" => !parts_form_only(raw, start, end),
            "circa" => circa_of(Some(raw)) != circa,
            "literal" => !same_text(raw, literal.as_deref()),
            "raw" => !same_text(raw, raw_text.as_deref()),
            _ => true,
        });
    }
    out
}

/// The members of a CSL name Pinakes types.
const NAME_PARTS: &[&str] = &[
    "family",
    "given",
    "non-dropping-particle",
    "dropping-particle",
    "suffix",
    "literal",
];

/// The top-level members read as text into a typed slot, `id` included.
const TYPED_TOP: &[&str] = &[
    "id",
    "type",
    "citation-key",
    "citation_key",
    "title",
    "title-short",
    "shortTitle",
    "container-title",
    "container-title-short",
    "journalAbbreviation",
    "collection-title",
    "publisher",
    "publisher-place",
    "volume",
    "issue",
    "number",
    "page",
    "edition",
    "genre",
    "abstract",
    "language",
    "note",
    "URL",
    "url",
    "DOI",
    "doi",
    "PMID",
    "pmid",
    "PMCID",
    "pmcid",
    "ISBN",
    "isbn",
    "ISSN",
    "issn",
];

/// The line each element of a top-level JSON array starts on. Empty when the text is not
/// an array.
fn item_lines(text: &str) -> Vec<usize> {
    let mut lines = Vec::new();
    let (mut depth, mut line, mut in_string, mut escaped, mut expect_item) =
        (0usize, 1usize, false, false, false);
    for c in text.chars() {
        if c == '\n' {
            line += 1;
        }
        if in_string {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        if depth == 1 && expect_item && !c.is_whitespace() && c != ']' {
            lines.push(line);
            expect_item = false;
        }
        match c {
            '"' => in_string = true,
            '[' | '{' => {
                depth += 1;
                if depth == 1 {
                    if c == '{' {
                        return Vec::new();
                    }
                    expect_item = true;
                }
            }
            ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 1 => expect_item = true,
            _ => {}
        }
    }
    lines
}

/// Read one CSL-JSON item. Fields Pinakes does not model go to `extra`.
/// `line` is where the item starts in the text, when it is known.
pub fn from_value(
    v: &Value,
    ordinal: usize,
    line: Option<usize>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Reference> {
    let at = |d: Diagnostic| {
        let d = d.record(ordinal);
        match line {
            Some(l) => d.at(l),
            None => d,
        }
    };
    let Value::Object(o) = v else {
        diags.push(at(Diagnostic::error(
            "csl.not-an-object",
            format!("Item {} is not a JSON object; skipped it", ordinal + 1),
        )));
        return None;
    };
    let mut r = Reference {
        id: o
            .get("id")
            .and_then(string_of)
            .unwrap_or_else(|| (ordinal + 1).to_string()),
        ..Reference::default()
    };
    for (k, val) in o {
        if k != "id"
            && TYPED_TOP.contains(&k.as_str())
            && !matches!(val, Value::String(_) | Value::Number(_) | Value::Null)
        {
            // CSL says these are strings. Keep the value as it came rather than guess.
            diags.push(at(Diagnostic::warning(
                "csl.wrong-type",
                format!("Item {}: {k:?} should be a string; kept it unread", r.id),
            )));
            r.extra.insert(k.clone(), val.clone());
            continue;
        }
        if matches!(k.as_str(), "author" | "editor") && !val.is_array() {
            diags.push(at(Diagnostic::warning(
                "csl.wrong-type",
                format!(
                    "Item {}: {k:?} should be a list of names; read it as one name",
                    r.id
                ),
            )));
        }
        if TYPED_TOP.contains(&k.as_str()) {
            keep_raw(&mut r.extra, k, val);
        } else if k == "keyword" {
            // Kept only when the list would not write back as the same value.
            let list = keyword_list(val);
            if list.is_empty() || *val != Value::String(list.join(", ")) {
                r.extra.insert(k.clone(), val.clone());
            }
        }
        let s = || string_of(val);
        match k.as_str() {
            "id" => {}
            "type" => r.kind = s().unwrap_or_default(),
            "citation-key" | "citation_key" => r.citation_key = s(),
            "title" => r.title = s(),
            "title-short" | "shortTitle" => r.title_short = s(),
            "container-title" => r.container_title = s(),
            "container-title-short" | "journalAbbreviation" => r.container_title_short = s(),
            "collection-title" => r.collection_title = s(),
            "publisher" => r.publisher = s(),
            "publisher-place" => r.publisher_place = s(),
            "volume" => r.volume = s(),
            "issue" => r.issue = s(),
            "number" => r.number = s(),
            "page" => r.page = s(),
            "edition" => r.edition = s(),
            "genre" => r.genre = s(),
            "abstract" => r.abstract_text = s(),
            "language" => r.language = s(),
            "note" => r.note = s(),
            "URL" | "url" => r.url = s(),
            "DOI" | "doi" => r.doi = s(),
            "PMID" | "pmid" => r.pmid = s(),
            "PMCID" | "pmcid" => r.pmcid = s(),
            "ISBN" | "isbn" => r.isbn = s(),
            "ISSN" | "issn" => r.issn = s(),
            "keyword" => r.keywords = keyword_list(val),
            "author" | "editor" => {
                let names: Vec<Name> = match val {
                    Value::Array(a) => a.iter().filter_map(parse_name_value).collect(),
                    other => parse_name_value(other).into_iter().collect(),
                };
                let whole = matches!(val, Value::Array(a) if a.len() == names.len()
                    && a.iter().all(Value::is_object));
                if !whole {
                    // Some of it was not a name object: keep the member as it came, which the
                    // writer puts back while the names are unchanged.
                    r.extra.insert(k.clone(), val.clone());
                }
                if k == "author" {
                    r.author = names;
                } else {
                    r.editor = names;
                }
            }
            "issued" | "accessed" => {
                let d = parse_date_value(val);
                if d.is_none() && !val.is_null() {
                    // Nothing to read in it, but it is still the item's: write it back as it came.
                    r.extra.insert(k.clone(), val.clone());
                }
                if k == "issued" {
                    r.issued = d;
                } else {
                    r.accessed = d;
                }
            }
            "custom" => {
                if let Value::Object(c) = val {
                    let mut rest = Map::new();
                    for (ck, cv) in c {
                        match ck.as_str() {
                            "arxiv" | "accession-number" | "database" => {
                                keep_raw(&mut rest, ck, cv);
                                let v = string_of(cv);
                                match ck.as_str() {
                                    "arxiv" => r.arxiv = v,
                                    "accession-number" => r.accession = v,
                                    _ => r.database = v,
                                }
                            }
                            "publication-type" => {
                                r.publication_types = publication_types_of(cv);
                                if *cv != json!(r.publication_types) {
                                    rest.insert(ck.clone(), cv.clone());
                                }
                            }
                            "source-fields" => {
                                r.source_fields = source_fields_of(cv);
                                if !plain_source_fields(cv) {
                                    // Something in it is not a source field as Pinakes writes
                                    // one: keep the member as it came.
                                    rest.insert(ck.clone(), cv.clone());
                                }
                            }
                            _ => {
                                rest.insert(ck.clone(), cv.clone());
                            }
                        }
                    }
                    if !rest.is_empty() {
                        r.extra.insert("custom".into(), Value::Object(rest));
                    }
                } else {
                    r.extra.insert(k.clone(), val.clone());
                }
            }
            _ => {
                r.extra.insert(k.clone(), val.clone());
            }
        }
    }
    if r.kind.is_empty() {
        diags.push(at(Diagnostic::warning(
            "csl.no-type",
            format!("Item {} has no type; wrote it as \"document\"", r.id),
        )));
        r.kind = "document".into();
    }
    crate::normalise::finish(&mut r, ordinal, line, diags);
    Some(r)
}

/// A key that appeared twice in one JSON object.
struct Repeat {
    /// The item it is in, counted from 0, if it is in one.
    item: Option<usize>,
    /// Whether it is in the document object, which is an item only when it is not a wrapper.
    top: bool,
    /// The item's member it is in, or the key itself at the item's top.
    member: String,
    key: String,
}

/// Where the parser is: what the value being read is, in CSL-JSON terms.
#[derive(Clone)]
enum Place {
    /// The whole document: a list of items, an `{"items": [...]}` wrapper or one item.
    Document,
    /// The `items` list of a wrapper.
    Items,
    /// Item `n`.
    Item(usize),
    /// Under member `m` of the document object, which is item 0 unless it is a wrapper.
    Top(String),
    /// Inside item `n` (if any), under its member `m`.
    Inside(Option<usize>, String),
}

/// A JSON value read the way `serde_json` reads it, last value winning for a repeated key,
/// with each repeated key noted.
struct Seed<'a> {
    place: Place,
    repeats: &'a std::cell::RefCell<Vec<Repeat>>,
}

impl<'de> serde::de::DeserializeSeed<'de> for Seed<'_> {
    type Value = Value;

    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> serde::de::Visitor<'de> for Seed<'_> {
    type Value = Value;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::from(v))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::from(v))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number))
    }

    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_string()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut out = Vec::new();
        loop {
            let place = match &self.place {
                Place::Document | Place::Items => Place::Item(out.len()),
                Place::Item(n) => Place::Inside(Some(*n), String::new()),
                Place::Top(m) => Place::Top(m.clone()),
                inside => inside.clone(),
            };
            let seed = Seed {
                place,
                repeats: self.repeats,
            };
            match seq.next_element_seed(seed)? {
                Some(v) => out.push(v),
                None => return Ok(Value::Array(out)),
            }
        }
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut out = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            let place = match &self.place {
                Place::Document if key == "items" => Place::Items,
                Place::Document => Place::Top(key.clone()),
                Place::Items => Place::Inside(None, key.clone()),
                Place::Item(n) => Place::Inside(Some(*n), key.clone()),
                Place::Inside(n, m) if m.is_empty() => Place::Inside(*n, key.clone()),
                Place::Top(m) => Place::Top(m.clone()),
                inside => inside.clone(),
            };
            if out.contains_key(&key) {
                let (item, top, member) = match &place {
                    Place::Inside(n, m) => (*n, false, m.clone()),
                    Place::Top(m) => (Some(0), true, m.clone()),
                    _ => (None, matches!(self.place, Place::Document), key.clone()),
                };
                self.repeats.borrow_mut().push(Repeat {
                    item,
                    top,
                    member,
                    key: key.clone(),
                });
            }
            let v = map.next_value_seed(Seed {
                place,
                repeats: self.repeats,
            })?;
            out.insert(key, v);
        }
        Ok(Value::Object(out))
    }
}

/// Read a CSL-JSON document: an array of items, `{"items": [...]}`, or one item.
pub fn read(text: &str, diags: &mut Vec<Diagnostic>) -> Vec<Reference> {
    let repeats = std::cell::RefCell::new(Vec::new());
    let parsed = {
        use serde::de::DeserializeSeed;
        let mut de = serde_json::Deserializer::from_str(text);
        Seed {
            place: Place::Document,
            repeats: &repeats,
        }
        .deserialize(&mut de)
        .and_then(|v| de.end().map(|()| v))
    };
    let value: Value = match parsed {
        Ok(v) => v,
        Err(e) => {
            diags.push(
                Diagnostic::error("csl.bad-json", format!("Not valid JSON: {e}")).at(e.line()),
            );
            return Vec::new();
        }
    };
    let items: Vec<&Value> = match &value {
        Value::Array(a) => a.iter().collect(),
        Value::Object(o) => match o.get("items") {
            Some(Value::Array(a)) => a.iter().collect(),
            _ => vec![&value],
        },
        _ => {
            diags.push(Diagnostic::error(
                "csl.not-a-list",
                "CSL-JSON must be a list of items or one item",
            ));
            Vec::new()
        }
    };
    let lines = item_lines(text);
    let single =
        matches!(value, Value::Object(_)) && !matches!(value.get("items"), Some(Value::Array(_)));
    let wrapper =
        matches!(&value, Value::Object(o) if matches!(o.get("items"), Some(Value::Array(_))));
    for rep in repeats.into_inner() {
        // In a wrapper, a key repeated beside `items` belongs to no item.
        let item = rep.item.filter(|_| !(wrapper && rep.top));
        let mut d = Diagnostic::warning(
            "csl.duplicate-key",
            match item {
                Some(n) if rep.member == rep.key => format!(
                    "Item {}: {:?} is given more than once; read the last value",
                    n + 1,
                    rep.key
                ),
                Some(n) => format!(
                    "Item {}: {:?} inside {:?} is given more than once; read the last value",
                    n + 1,
                    rep.key,
                    rep.member
                ),
                None => format!("{:?} is given more than once; read the last value", rep.key),
            },
        );
        if let Some(n) = item {
            d = d.record(n).field(rep.member.clone());
            if let Some(l) = lines.get(n).copied().or(single.then_some(1)) {
                d = d.at(l);
            }
        }
        diags.push(d);
    }
    items
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| {
            let line = lines.get(i).copied().or(single.then_some(1));
            from_value(v, i, line, diags)
        })
        .collect()
}

/// Write CSL-JSON: a pretty-printed array. A byte-order mark can only be inside a string, and
/// is written as the escape `\ufeff`, because reading removes the character itself.
pub fn write(refs: &[Reference]) -> String {
    let items: Vec<Value> = refs.iter().map(to_value).collect();
    let mut out =
        serde_json::to_string_pretty(&Value::Array(items)).unwrap_or_else(|_| "[]".into());
    if out.contains('\u{feff}') {
        out = out.replace('\u{feff}', "\\ufeff");
    }
    out.push('\n');
    out
}
