//! No value breaks a writer's framing, and what a writer can represent reads back exactly.
//!
//! Framing: any reference, with any text in any field, writes as exactly one record that reads
//! back without an error. Exactness: a reference in normal form reads back with every text field
//! the format holds unchanged. DESIGN.md lists what each format cannot hold.
//!
//! CSL-JSON goes the other way too: any CSL-shaped object, unknown members included, reads and
//! writes back as it came.

use pinakes::{normalise, read, write, Date, Format, Name, Reference, Severity, SourceField};
use proptest::prelude::*;
use serde_json::{json, Map, Value};

/// Pieces that each format's framing gives meaning to, and some that look like them.
const HOSTILE: &[&str] = &[
    "{",
    "}",
    "\\",
    "$",
    "%",
    "#",
    "&",
    "_",
    "~",
    "^",
    "@",
    ",",
    ";",
    "\"",
    "'",
    "`",
    "--",
    "-",
    "\n",
    "\r\n",
    "\r",
    "\n\n",
    "\t",
    " ",
    "  ",
    "TY  - ",
    "ER  - ",
    "\nER  - \n",
    "\nTY  - JOUR",
    "\n@article{x,",
    "\n@",
    "and",
    " and ",
    "others",
    ":",
    ": ",
    "---",
    "...",
    "- ",
    "\n- ",
    "#",
    "<i>",
    "</i>",
    "<b>",
    "<sup>",
    "\\par",
    "\\url{",
    "\\emph{",
    "yes",
    "null",
    "NaN",
    "~",
    "*",
    "&amp;",
    "&#x41;",
    "|",
    ">",
    "!",
    "?",
    "[",
    "]",
    "\u{2028}",
    "\u{feff}",
    "%20",
    "{\\'e}",
    "é",
    "ß",
    "μ",
    "µ",
    "–",
    "—",
    "“",
    "”",
    "‘",
    "’",
    "\u{0}",
    "\u{1}",
    "\u{7f}",
    "\u{85}",
    "0",
    "1",
    "12",
    "x",
    "Ab",
    "van",
    "de",
    "Jr.",
    "=",
    "(",
    ")",
    "/",
    "\\\\",
    "{}",
    "}{",
];

fn hostile() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            3 => prop::sample::select(HOSTILE).prop_map(str::to_string),
            2 => "[A-Za-z]{1,7}",
            1 => any::<char>().prop_map(String::from),
        ],
        0..8,
    )
    .prop_map(|p| p.concat())
}

fn maybe() -> impl Strategy<Value = Option<String>> {
    prop::option::weighted(0.7, hostile())
}

fn name() -> impl Strategy<Value = Name> {
    prop_oneof![
        3 => (maybe(), maybe(), maybe(), maybe(), maybe()).prop_map(|(f, g, p, d, s)| Name {
            family: f.or(Some("Lind".into())),
            given: g,
            particle: p,
            dropping_particle: d,
            suffix: s,
            ..Name::default()
        }),
        1 => hostile().prop_map(|l| Name::literal(&l)),
        1 => Just(Name::others()),
    ]
}

const KINDS: &[&str] = &[
    "article-journal",
    "book",
    "chapter",
    "report",
    "thesis",
    "paper-conference",
    "webpage",
    "document",
    "dataset",
];

/// A reference with arbitrary text in every field.
fn reference() -> impl Strategy<Value = Reference> {
    (
        prop::collection::vec(maybe(), 26),
        prop::collection::vec(name(), 0..4),
        prop::collection::vec(name(), 0..3),
        prop::collection::vec(hostile(), 0..4),
        prop::option::of((
            -50i32..10000,
            prop::option::of(1u8..=12),
            prop::option::of(1u8..=28),
        )),
        prop::sample::select(KINDS),
        hostile(),
        prop::collection::vec((hostile(), hostile()), 0..3),
    )
        .prop_map(|(t, author, editor, keywords, date, kind, id, fields)| {
            let mut t = t.into_iter();
            let mut n = || t.next().unwrap();
            Reference {
                id,
                kind: kind.to_string(),
                citation_key: n(),
                title: n(),
                title_short: n(),
                author,
                editor,
                issued: date.map(|(y, m, d)| Date::ymd(y, m, m.and(d))),
                container_title: n(),
                container_title_short: n(),
                collection_title: n(),
                publisher: n(),
                publisher_place: n(),
                volume: n(),
                issue: n(),
                number: n(),
                page: n(),
                edition: n(),
                genre: n(),
                abstract_text: n(),
                language: n(),
                note: n(),
                keywords,
                url: n(),
                doi: n(),
                pmid: n(),
                pmcid: n(),
                isbn: n(),
                issn: n(),
                arxiv: n(),
                accession: n(),
                database: n(),
                source_fields: fields
                    .into_iter()
                    .flat_map(|(name, value)| {
                        ["ris", "bibtex"].map(|format| SourceField {
                            format: format.into(),
                            name: name.clone(),
                            value: value.clone(),
                        })
                    })
                    .collect(),
                ..Reference::default()
            }
        })
}

fn errors(lib: &pinakes::Library) -> Vec<String> {
    lib.diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| format!("{}: {}", d.code, d.message))
        .collect()
}

const TEXT_FORMATS: [Format; 3] = [Format::Ris, Format::Bibtex, Format::Biblatex];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Two records with any text in them write as two records.
    #[test]
    fn no_value_breaks_the_framing(a in reference(), b in reference()) {
        let refs = [a, b];
        for f in TEXT_FORMATS {
            let text = write(&refs, f).unwrap();
            let back = read(text.as_bytes(), Some(f));
            prop_assert_eq!(back.references.len(), 2, "{}\n{}\n{:?}", f, text, back.diagnostics);
            prop_assert!(errors(&back).is_empty(), "{}\n{}\n{:?}", f, text, errors(&back));
        }
        let yaml = write(&refs, Format::Hayagriva).unwrap();
        let lib = hayagriva::io::from_yaml_str(&yaml);
        prop_assert!(lib.is_ok(), "hayagriva rejected\n{}\n{:?}", yaml, lib.err());
        prop_assert_eq!(lib.unwrap().len(), 2, "{}", yaml);
    }

    /// A reference in normal form keeps every text field the format holds.
    #[test]
    fn text_survives_write_and_read(r in reference()) {
        let r = normal(r);
        for f in TEXT_FORMATS {
            let text = write(std::slice::from_ref(&r), f).unwrap();
            let back = read(text.as_bytes(), Some(f));
            prop_assert_eq!(back.references.len(), 1, "{}\n{}", f, text);
            let b = &back.references[0];
            prop_assert_eq!(held(&r, f), held(b, f), "{}\n{}", f, text);
            if f == Format::Ris {
                // RIS names go through the RIS name reader, which reads exports' habits
                // (Vancouver initials, a trailing ";"). Every name survives, and one pass
                // reaches the form that then stays.
                // Only a name of nothing but commas and semicolons is gone.
                let count = |names: &[Name]| {
                    names
                        .iter()
                        .filter(|n| !n.inverted().trim_matches([',', ';', ' ']).is_empty())
                        .count()
                };
                prop_assert_eq!(count(&r.author), b.author.len(), "{}", text);
                prop_assert_eq!(count(&r.editor), b.editor.len(), "{}", text);
                let again = read(write(std::slice::from_ref(b), f).unwrap().as_bytes(), Some(f));
                prop_assert_eq!(&again.references[0].author, &b.author);
                prop_assert_eq!(&again.references[0].editor, &b.editor);
            }
        }
        let yaml = write(std::slice::from_ref(&r), Format::Hayagriva).unwrap();
        let lib = hayagriva::io::from_yaml_str(&yaml);
        prop_assert!(lib.is_ok(), "hayagriva rejected\n{}\n{:?}", yaml, lib.err());
        let lib = lib.unwrap();
        let entry = lib.iter().next().unwrap();
        for (ours, theirs) in hayagriva_held(&r).into_iter().zip(hayagriva_fields(entry)) {
            // Hayagriva drops markup from titles and abstracts.
            if !(matches!(ours.0, "title" | "abstract") && ours.1.contains('<')) {
                prop_assert_eq!(ours, theirs, "{}", yaml);
            }
        }
    }

    /// Any CSL-shaped item, with unknown members in it, its names and its dates, reads and
    /// writes back as it came.
    #[test]
    fn csl_shaped_objects_survive(items in prop::collection::vec(csl_item(), 1..4)) {
        let input = Value::Array(items);
        let text = serde_json::to_string_pretty(&input).unwrap();
        let lib = read(text.as_bytes(), Some(Format::CslJson));
        prop_assert!(errors(&lib).is_empty(), "{:?}", errors(&lib));
        let out: Value = serde_json::from_str(&write(&lib.references, Format::CslJson).unwrap()).unwrap();
        prop_assert_eq!(out, input);
    }
}

/// The reference as a reader would leave it: normal form, no identifiers (they have their own
/// tests), no source fields.
fn normal(mut r: Reference) -> Reference {
    r.doi = None;
    r.url = None;
    r.pmid = None;
    r.pmcid = None;
    r.isbn = None;
    r.issn = None;
    r.arxiv = None;
    r.accession = None;
    r.source_fields.clear();
    normalise(&mut r);
    r
}

/// The text fields a format holds, as a reader of it would compare them.
fn held(r: &Reference, f: Format) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let mut put =
        |k: &'static str, v: &Option<String>| out.push((k, v.clone().unwrap_or_default()));
    put("title", &r.title);
    put("container-title", &r.container_title);
    put("publisher", &r.publisher);
    put("publisher-place", &r.publisher_place);
    put("volume", &r.volume);
    put("issue", &r.issue);
    put("number", &r.number);
    put("page", &r.page);
    put("edition", &r.edition);
    put("abstract", &r.abstract_text);
    put("note", &r.note);
    put("language", &r.language);
    // Classic BibTeX has no thesis without a degree: @phdthesis implies one.
    let implied = (f == Format::Bibtex && r.kind == "thesis" && r.genre.is_none())
        .then(|| "PhD thesis".to_string());
    // RIS readers take an M3 of up to eight digits for a PMID, as PubMed-derived exports write.
    let pmid_like = |g: &String| g.len() <= 8 && g.chars().all(|c| c.is_ascii_digit());
    let genre = r
        .genre
        .clone()
        .filter(|g| !(f == Format::Ris && pmid_like(g)));
    put("genre", &genre.or(implied));
    put("title-short", &r.title_short);
    put("container-title-short", &r.container_title_short);
    put("collection-title", &r.collection_title);
    if f == Format::Ris {
        put("database", &r.database);
    }
    out.push(("keywords", r.keywords.join("|")));
    if f != Format::Ris {
        for n in r.author.iter().chain(&r.editor) {
            out.push(("name", name_text(n)));
        }
    }
    out
}

/// A BibTeX name: the particle is only told apart from the family name by its case.
fn name_text(n: &Name) -> String {
    format!(
        "{:?}|{:?}|{:?}|{:?}",
        n.literal,
        [
            n.dropping_particle.as_deref(),
            n.particle.as_deref(),
            n.family.as_deref()
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" "),
        n.given,
        n.suffix
    )
}

fn hayagriva_held(r: &Reference) -> Vec<(&'static str, String)> {
    let text = |v: &Option<String>| v.clone().unwrap_or_default();
    vec![
        ("title", text(&r.title)),
        ("abstract", text(&r.abstract_text)),
        ("note", r.note.clone().unwrap_or_default()),
        ("genre", r.genre.clone().unwrap_or_default()),
        (
            "authors",
            r.author
                .iter()
                .map(|n| {
                    let family = n
                        .literal
                        .clone()
                        .unwrap_or_else(|| n.family.clone().unwrap_or_default());
                    let prefix = [n.dropping_particle.as_deref(), n.particle.as_deref()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" ");
                    format!("{family}|{:?}|{prefix}|{:?}", n.given, n.suffix)
                })
                .collect::<Vec<_>>()
                .join(";"),
        ),
    ]
}

fn hayagriva_fields(e: &hayagriva::Entry) -> Vec<(&'static str, String)> {
    let s =
        |v: Option<&hayagriva::types::FormatString>| v.map(|v| v.to_string()).unwrap_or_default();
    vec![
        ("title", s(e.title())),
        ("abstract", s(e.abstract_())),
        ("note", s(e.note())),
        ("genre", s(e.genre())),
        (
            "authors",
            e.authors()
                .unwrap_or_default()
                .iter()
                .map(|p| {
                    format!(
                        "{}|{:?}|{}|{:?}",
                        p.name,
                        p.given_name,
                        p.prefix.clone().unwrap_or_default(),
                        p.suffix
                    )
                })
                .collect::<Vec<_>>()
                .join(";"),
        ),
    ]
}

// --- CSL-shaped items ---

fn word() -> impl Strategy<Value = String> {
    "[A-Za-z]{1,8}( [A-Za-z]{1,8}){0,2}"
}

/// Any JSON value, a few levels deep.
fn any_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i32>().prop_map(|n| json!(n)),
        (-1e6f64..1e6).prop_map(|n| json!(n)),
        "\\PC{0,10}".prop_map(Value::String),
    ];
    leaf.prop_recursive(3, 12, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::btree_map("[a-z-]{1,8}", inner, 0..4)
                .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

/// Members no reader types: CSL's own (`static-ordering`, `season`) and anything else.
fn unknown_members(known: &'static [&'static str]) -> impl Strategy<Value = Map<String, Value>> {
    prop::collection::btree_map(
        prop_oneof![
            prop::sample::select(
                &[
                    "static-ordering",
                    "comma-suffix",
                    "parse-names",
                    "season",
                    "x-shelf",
                    "multi",
                    "_note",
                    "title-alt",
                ][..]
            )
            .prop_map(str::to_string),
            "x-[a-z]{1,6}",
        ],
        any_json(),
        0..3,
    )
    .prop_map(move |m| {
        m.into_iter()
            .filter(|(k, _)| !known.contains(&k.as_str()))
            .collect()
    })
}

fn text_or_number() -> impl Strategy<Value = Value> {
    prop_oneof![
        3 => word().prop_map(Value::String),
        1 => (1u32..5000).prop_map(|n| json!(n)),
    ]
}

fn csl_name() -> impl Strategy<Value = Value> {
    (
        prop::option::of(word()),
        prop::option::of(word()),
        prop::option::of(word()),
        prop::option::of(word()),
        unknown_members(&[]),
    )
        .prop_map(|(family, given, particle, literal, extra)| {
            let mut m = extra;
            if let Some(l) = literal {
                m.insert("literal".into(), json!(l));
            } else {
                m.insert(
                    "family".into(),
                    json!(family.unwrap_or_else(|| "Lind".into())),
                );
                if let Some(g) = given {
                    m.insert("given".into(), json!(g));
                }
                if let Some(p) = particle {
                    m.insert("non-dropping-particle".into(), json!(p.to_lowercase()));
                }
            }
            Value::Object(m)
        })
}

fn csl_date() -> impl Strategy<Value = Value> {
    (
        prop::collection::vec(
            prop::collection::vec(
                prop_oneof![
                    (1..2100i32).prop_map(|n| json!(n)),
                    (1..13i32).prop_map(|n| json!(n.to_string())),
                ],
                1..4,
            ),
            0..3,
        ),
        prop::option::of(prop_oneof![
            Just(json!(true)),
            Just(json!(false)),
            Just(json!(1)),
        ]),
        prop::option::of("[a-z]{2,8}( [a-z]{2,8})?"),
        unknown_members(&["date-parts", "circa", "literal", "raw"]),
    )
        .prop_map(|(parts, circa, literal, extra)| {
            let mut m = extra;
            if !parts.is_empty() {
                m.insert("date-parts".into(), json!(parts));
            }
            if let Some(c) = circa {
                m.insert("circa".into(), c);
            }
            if let Some(l) = literal {
                m.insert("literal".into(), json!(l));
            }
            Value::Object(m)
        })
}

const CSL_TEXT: &[&str] = &[
    "title",
    "container-title",
    "publisher",
    "publisher-place",
    "volume",
    "issue",
    "number",
    "page",
    "edition",
    "genre",
    "abstract",
    "note",
    "collection-title",
];

fn csl_item() -> impl Strategy<Value = Value> {
    (
        prop_oneof![
            "[a-z0-9]{1,8}".prop_map(Value::String),
            (0u32..1000).prop_map(|n| json!(n))
        ],
        prop::sample::select(KINDS),
        prop::collection::btree_map(prop::sample::select(CSL_TEXT), text_or_number(), 0..6),
        prop::collection::vec(csl_name(), 0..3),
        prop::option::of(csl_date()),
        prop::option::of(prop_oneof![
            prop::collection::vec("[a-z]{1,8}", 1..3).prop_map(|k| json!(k)),
            "[a-z]{1,8}(, [a-z]{1,8})?".prop_map(Value::String),
        ]),
        unknown_members(&[
            "id",
            "type",
            "author",
            "issued",
            "keyword",
            "custom",
            "title-alt",
        ]),
        prop::option::of(unknown_members(&[])),
    )
        .prop_map(|(id, kind, text, author, issued, keyword, extra, custom)| {
            let mut m = extra;
            m.insert("id".into(), id);
            m.insert("type".into(), json!(kind));
            for (k, v) in text {
                m.insert(k.to_string(), v);
            }
            if !author.is_empty() {
                m.insert("author".into(), Value::Array(author));
            }
            if let Some(d) = issued {
                m.insert("issued".into(), d);
            }
            if let Some(k) = keyword {
                m.insert("keyword".into(), k);
            }
            if let Some(c) = custom.filter(|c| !c.is_empty()) {
                m.insert("custom".into(), Value::Object(c));
            }
            Value::Object(m)
        })
}
