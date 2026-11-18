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
    let mut custom = match r.extra.get("custom") {
        Some(Value::Object(c)) => c.clone(),
        _ => Map::new(),
    };
    if let Some(a) = &r.arxiv {
        custom.insert("arxiv".into(), Value::String(a.clone()));
    }
    if let Some(a) = &r.accession {
        custom.insert("accession-number".into(), Value::String(a.clone()));
    }
    if let Some(a) = &r.database {
        custom.insert("database".into(), Value::String(a.clone()));
    }
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
    for (k, v) in &r.extra {
        if k != "custom" && !m.contains_key(k) {
            m.insert(k.clone(), v.clone());
        }
    }
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
    Value::Object(m)
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
            let n = Name {
                family: get("family"),
                given: get("given"),
                particle: get("non-dropping-particle"),
                dropping_particle: get("dropping-particle"),
                suffix: get("suffix"),
                literal: get("literal"),
            };
            (!n.is_empty()).then_some(n)
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
            if let Some(Value::Array(parts)) = o.get("date-parts") {
                d.start = parts.first().and_then(parse_ymd);
                d.end = parts.get(1).and_then(parse_ymd);
            }
            d.circa = matches!(
                o.get("circa"),
                Some(Value::Bool(true)) | Some(Value::Number(_))
            ) || matches!(o.get("circa"), Some(Value::String(s)) if !s.is_empty());
            d.literal = o.get("literal").and_then(string_of);
            d.raw = o.get("raw").and_then(string_of);
            if d.start.is_none() {
                if let Some(raw) = d.raw.clone().or_else(|| d.literal.clone()) {
                    if let Some(parsed) = parse_date(&raw) {
                        if parsed.start.is_some() {
                            d.start = parsed.start;
                            d.end = parsed.end;
                            d.literal = None;
                            d.raw = Some(raw);
                        }
                    }
                }
            }
            (d != Date::default()).then_some(d)
        }
        _ => None,
    }
}

/// The CSL fields Pinakes reads as text.
const KNOWN_TEXT: &[&str] = &[
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
        if KNOWN_TEXT.contains(&k.as_str())
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
            "keyword" => {
                r.keywords = match val {
                    Value::Array(a) => a.iter().filter_map(string_of).collect(),
                    Value::String(s) => s
                        .split([',', ';'])
                        .filter_map(crate::text::non_empty)
                        .collect(),
                    _ => Vec::new(),
                }
            }
            "author" | "editor" => {
                let names: Vec<Name> = match val {
                    Value::Array(a) => a.iter().filter_map(parse_name_value).collect(),
                    other => parse_name_value(other).into_iter().collect(),
                };
                if k == "author" {
                    r.author = names;
                } else {
                    r.editor = names;
                }
            }
            "issued" => r.issued = parse_date_value(val),
            "accessed" => r.accessed = parse_date_value(val),
            "custom" => {
                if let Value::Object(c) = val {
                    let mut rest = Map::new();
                    for (ck, cv) in c {
                        match ck.as_str() {
                            "arxiv" => r.arxiv = string_of(cv),
                            "accession-number" => r.accession = string_of(cv),
                            "database" => r.database = string_of(cv),
                            "publication-type" => {
                                r.publication_types = match cv {
                                    Value::Array(a) => a.iter().filter_map(string_of).collect(),
                                    other => string_of(other).into_iter().collect(),
                                }
                            }
                            "source-fields" => {
                                if let Value::Array(a) = cv {
                                    for f in a {
                                        let get = |key: &str| {
                                            f.get(key)
                                                .and_then(Value::as_str)
                                                .unwrap_or_default()
                                                .to_string()
                                        };
                                        r.source_fields.push(SourceField {
                                            format: get("format"),
                                            name: get("name"),
                                            value: get("value"),
                                        });
                                    }
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

/// Read a CSL-JSON document: an array of items, `{"items": [...]}`, or one item.
pub fn read(text: &str, diags: &mut Vec<Diagnostic>) -> Vec<Reference> {
    let value: Value = match serde_json::from_str(text) {
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
    items
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| {
            let line = lines.get(i).copied().or(single.then_some(1));
            from_value(v, i, line, diags)
        })
        .collect()
}

/// Write CSL-JSON: a pretty-printed array.
pub fn write(refs: &[Reference]) -> String {
    let items: Vec<Value> = refs.iter().map(to_value).collect();
    let mut out =
        serde_json::to_string_pretty(&Value::Array(items)).unwrap_or_else(|_| "[]".into());
    out.push('\n');
    out
}
