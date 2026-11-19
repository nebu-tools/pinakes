//! What writers and readers report, case by case. `writers.rs` checks the same with generated
//! records; these are the cases written out, so a change in one shows by name.

use pinakes::{read, write, Diagnostic, Format, Reference, Severity};
use serde_json::{json, Value};

fn from_csl(item: Value) -> Vec<Reference> {
    let lib = read(json!([item]).to_string().as_bytes(), Some(Format::CslJson));
    assert!(lib.diagnostics.is_empty(), "{:?}", lib.diagnostics);
    lib.references
}

/// The diagnostics with this code.
fn with_code<'a>(diags: &'a [Diagnostic], code: &str) -> Vec<&'a Diagnostic> {
    diags.iter().filter(|d| d.code == code).collect()
}

/// Write to `format`, read back, and return the text, the writer's diagnostics and the item as
/// CSL-JSON.
fn round_trip(refs: &[Reference], format: Format) -> (String, Vec<Diagnostic>, Value) {
    let out = write(refs, format).unwrap();
    let back = read(out.text.as_bytes(), Some(format));
    let csl: Value =
        serde_json::from_str(&write(&back.references, Format::CslJson).unwrap().text).unwrap();
    (out.text, out.diagnostics, csl[0].clone())
}

#[test]
fn a_url_that_bibtex_can_hold_is_written_exactly() {
    for url in [
        "https://example.org/a%23_~\\{x\\}",
        "https://example.org/a{b}c",
        "https://example.org/?q=a&b=$1#x",
    ] {
        let refs = from_csl(json!({"id": "a", "type": "book", "title": "T", "URL": url}));
        for format in [Format::Bibtex, Format::Biblatex] {
            let (text, diags, back) = round_trip(&refs, format);
            assert!(text.contains(&format!("= {{{url}}},\n")), "{text}");
            assert!(diags.is_empty(), "{diags:?}");
            assert_eq!(back["URL"], url);
        }
    }
}

#[test]
fn a_url_that_bibtex_cannot_hold_is_percent_encoded_and_reported() {
    for (url, written) in [
        ("https://example.org/a}b", "https://example.org/a%7Db"),
        ("https://example.org/a\\", "https://example.org/a%5C"),
        ("https://example.org/a{b", "https://example.org/a%7Bb"),
        ("https://example.org/}{", "https://example.org/%7D%7B"),
    ] {
        let refs = from_csl(json!({"id": "a", "type": "book", "title": "T", "URL": url}));
        for format in [Format::Bibtex, Format::Biblatex] {
            let (text, diags, back) = round_trip(&refs, format);
            assert!(text.contains(&format!("= {{{written}}},\n")), "{text}");
            let d = with_code(&diags, "bibtex.verbatim-encoded");
            assert_eq!(d.len(), 1, "{diags:?}");
            assert_eq!(d[0].field.as_deref(), Some("URL"));
            assert_eq!(d[0].record, Some(0));
            assert_eq!(d[0].severity, Severity::Warning);
            // The reader does not decode percent escapes.
            assert_eq!(back["URL"], written);
        }
    }
}

#[test]
fn a_doi_that_bibtex_cannot_hold_is_percent_encoded_and_reported() {
    let refs = from_csl(json!({"id": "a", "type": "book", "title": "T", "DOI": "10.5555/a}b"}));
    let (text, diags, _) = round_trip(&refs, Format::Biblatex);
    assert!(text.contains("= {10.5555/a%7Db},\n"), "{text}");
    let d = with_code(&diags, "bibtex.verbatim-encoded");
    assert_eq!(d.len(), 1, "{diags:?}");
    assert_eq!(d[0].field.as_deref(), Some("DOI"));
}

#[test]
fn another_verbatim_field_keeps_the_file_well_formed_and_is_reported() {
    let refs = from_csl(json!({
        "id": "a", "type": "book", "title": "T",
        "custom": {"source-fields": [{"format": "bibtex", "name": "file", "value": "a}{b"}]}
    }));
    let (text, diags, _) = round_trip(&refs, Format::Biblatex);
    assert!(text.contains("file  = {ab},\n"), "{text}");
    let d = with_code(&diags, "bibtex.verbatim-changed");
    assert_eq!(d.len(), 1, "{diags:?}");
    assert_eq!(d[0].field.as_deref(), Some("custom.source-fields"));
}

#[test]
fn csl_json_keeps_unknown_members_inside_source_fields() {
    let item = json!({
        "id": "a", "type": "book", "title": "T",
        "custom": {"source-fields": [
            {"format": "ris", "name": "XY", "value": "z", "unknown": 1}
        ]}
    });
    let lib = read(json!([item]).to_string().as_bytes(), Some(Format::CslJson));
    assert!(lib.diagnostics.is_empty(), "{:?}", lib.diagnostics);
    let r = &lib.references[0];
    assert_eq!(r.source_fields.len(), 1);
    assert_eq!(r.source_fields[0].value, "z");
    let out = write(&lib.references, Format::CslJson).unwrap();
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    let back: Value = serde_json::from_str(&out.text).unwrap();
    assert_eq!(back, json!([item]));
}

#[test]
fn csl_json_keeps_source_field_text_exactly() {
    let item = json!({
        "id": "a", "type": "book", "title": "T",
        "custom": {"source-fields": [
            {"format": "ris", "name": "TY  - ", "value": " z "},
            {"format": "ris", "name": "N1", "value": "a\u{feff}b"}
        ]}
    });
    // Written out, the mark would be removed as a stray one; the escape keeps it.
    let text = json!([item]).to_string().replace('\u{feff}', "\\ufeff");
    let lib = read(text.as_bytes(), Some(Format::CslJson));
    assert!(lib.diagnostics.is_empty(), "{:?}", lib.diagnostics);
    let refs = lib.references;
    assert_eq!(refs[0].source_fields[0].name, "TY  - ");
    assert_eq!(refs[0].source_fields[1].value, "a\u{feff}b");
    assert_eq!(refs[0].source_fields[0].value, " z ");
    let out = write(&refs, Format::CslJson).unwrap();
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert!(out.text.contains("a\\ufeffb"), "{}", out.text);
    let back = read(out.text.as_bytes(), Some(Format::CslJson));
    assert!(back.diagnostics.is_empty(), "{:?}", back.diagnostics);
    assert_eq!(back.references[0].source_fields, refs[0].source_fields);
}

#[test]
fn csl_json_keeps_unknown_members_at_every_level() {
    let item = json!({
        "id": "a", "type": "book", "title": "T",
        "x": {"a": [1, {"b": 2}]},
        "author": [{"family": "Doe", "x": {"b": 3}}],
        "issued": {"date-parts": [[2020]], "x": {"b": 4}},
        "custom": {"arxiv": 2101, "x": [true], "publication-type": ["Review", 3]}
    });
    let lib = read(json!([item]).to_string().as_bytes(), Some(Format::CslJson));
    let out = write(&lib.references, Format::CslJson).unwrap();
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_eq!(
        serde_json::from_str::<Value>(&out.text).unwrap(),
        json!([item])
    );
}

#[test]
fn a_duplicate_json_key_is_read_with_the_last_value_and_reported() {
    let text = "[\n{\"id\":\"a\",\"type\":\"book\",\"title\":\"T\",\"x\":1,\"x\":2},\n\
                {\"id\":\"b\",\"type\":\"book\",\"title\":\"U\",\
                \"author\":[{\"family\":\"A\",\"family\":\"B\"}]}\n]";
    let lib = read(text.as_bytes(), Some(Format::CslJson));
    assert_eq!(lib.references[0].extra["x"], json!(2));
    assert_eq!(lib.references[1].author[0].family.as_deref(), Some("B"));
    let d = with_code(&lib.diagnostics, "csl.duplicate-key");
    assert_eq!(d.len(), 2, "{:?}", lib.diagnostics);
    assert_eq!((d[0].record, d[0].field.as_deref()), (Some(0), Some("x")));
    assert_eq!(d[0].line, Some(2));
    assert!(d[0].message.contains("\"x\""), "{}", d[0].message);
    assert_eq!(
        (d[1].record, d[1].field.as_deref()),
        (Some(1), Some("author"))
    );
    assert!(d[1].message.contains("\"family\""), "{}", d[1].message);
}

#[test]
fn ris_reports_a_name_it_would_read_back_as_et_al() {
    for name in [
        json!({"literal": "et al."}),
        json!({"family": "others"}),
        json!({"literal": "others"}),
    ] {
        let refs = from_csl(json!({"id": "a", "type": "book", "title": "T", "author": [name]}));
        let out = write(&refs, Format::Ris).unwrap();
        let d = with_code(&out.diagnostics, "ris.et-al");
        // `{"literal": "others"}` is the marker itself: nothing to report.
        let marker = refs[0].author[0].is_others();
        assert_eq!(
            d.len(),
            usize::from(!marker),
            "{name}: {:?}",
            out.diagnostics
        );
        if let Some(d) = d.first() {
            assert_eq!(d.field.as_deref(), Some("author"));
            assert_eq!(d.record, Some(0));
        }
    }
    // A person named Others with a given name is not the marker.
    let refs = from_csl(json!({
        "id": "a", "type": "book", "title": "T",
        "author": [{"family": "Others", "given": "Alice"}]
    }));
    assert!(write(&refs, Format::Ris).unwrap().diagnostics.is_empty());
}

#[test]
fn writers_report_fields_they_have_no_place_for() {
    let refs = from_csl(json!({
        "id": "a", "type": "book", "title": "T", "x-shelf": "B4",
        "custom": {"source-fields": [{"format": "bibtex", "name": "owner", "value": "ada"}]}
    }));
    let ris = write(&refs, Format::Ris).unwrap().diagnostics;
    let fields: Vec<_> = with_code(&ris, "ris.left-out")
        .iter()
        .map(|d| d.field.as_deref().unwrap())
        .collect();
    assert_eq!(fields, ["x-shelf", "custom.source-fields"]);
    let bib = write(&refs, Format::Biblatex).unwrap().diagnostics;
    let fields: Vec<_> = with_code(&bib, "bibtex.left-out")
        .iter()
        .map(|d| d.field.as_deref().unwrap())
        .collect();
    assert_eq!(fields, ["x-shelf"]);
    assert!(write(&refs, Format::CslJson)
        .unwrap()
        .diagnostics
        .is_empty());
}

#[test]
fn bibtex_reports_an_implied_genre_and_ris_a_genre_read_as_a_pmid() {
    let refs = from_csl(json!({"id": "a", "type": "thesis", "title": "T"}));
    let d = write(&refs, Format::Bibtex).unwrap().diagnostics;
    let d = with_code(&d, "bibtex.changed");
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].field.as_deref(), Some("genre"));
    let refs = from_csl(json!({"id": "a", "type": "book", "title": "T", "genre": "123"}));
    let d = write(&refs, Format::Ris).unwrap().diagnostics;
    let fields: Vec<_> = d.iter().map(|d| d.field.as_deref().unwrap()).collect();
    assert!(
        fields.contains(&"genre") && fields.contains(&"PMID"),
        "{d:?}"
    );
}

#[test]
fn hayagriva_reports_what_it_cannot_hold() {
    let refs = from_csl(json!({
        "id": "a", "type": "book", "title": "Moths <i>at night</i>",
        "keyword": "moths", "title-short": "Moths",
        "author": [{"family": "Lind", "dropping-particle": "de"}]
    }));
    let d = write(&refs, Format::Hayagriva).unwrap().diagnostics;
    let codes: Vec<_> = d
        .iter()
        .map(|d| (d.code, d.field.as_deref().unwrap()))
        .collect();
    for want in [
        ("hayagriva.markup", "title"),
        ("hayagriva.left-out", "keyword"),
        ("hayagriva.left-out", "title-short"),
        ("hayagriva.name", "author"),
    ] {
        assert!(codes.contains(&want), "{want:?} not in {codes:?}");
    }
    assert!(d.iter().all(|d| d.record == Some(0)));
}

#[test]
fn a_clean_record_gives_no_writer_diagnostics() {
    let refs = from_csl(json!({
        "id": "adeyemi2019moths", "type": "article-journal", "title": "Moths of the forest",
        "author": [{"family": "Adeyemi", "given": "Kemi"}],
        "issued": {"date-parts": [[2019, 5]]}, "container-title": "Ecology",
        "volume": "4", "page": "1-9", "DOI": "10.5555/x"
    }));
    for format in [
        Format::Ris,
        Format::Bibtex,
        Format::Biblatex,
        Format::CslJson,
        Format::Hayagriva,
    ] {
        let d = write(&refs, format).unwrap().diagnostics;
        assert!(d.is_empty(), "{format}: {d:?}");
    }
}

#[test]
fn latex_beyond_the_depth_limit_is_kept_and_reported() {
    let text = format!(
        "@book{{a,\n  title={{{}T{}}}}}\n",
        "\\textit{".repeat(100),
        "}".repeat(100)
    );
    let lib = read(text.as_bytes(), Some(Format::Biblatex));
    let d = with_code(&lib.diagnostics, "bibtex.too-deep");
    assert_eq!(d.len(), 1, "{:?}", lib.diagnostics);
    assert_eq!(
        (d[0].record, d[0].field.as_deref(), d[0].line),
        (Some(0), Some("title"), Some(2))
    );
    let title = lib.references[0].title.as_deref().unwrap();
    assert!(title.contains("\\textit{\\textit{T}}"), "{title}");
    // The same title 64 deep is converted in full.
    let text = format!(
        "@book{{a,title={{{}T{}}}}}\n",
        "\\textit{".repeat(60),
        "}".repeat(60)
    );
    let lib = read(text.as_bytes(), Some(Format::Biblatex));
    assert!(lib.diagnostics.is_empty(), "{:?}", lib.diagnostics);
    assert!(!lib.references[0].title.as_deref().unwrap().contains('\\'));
}

#[test]
fn ris_keeps_a_citation_key_on_its_id_line() {
    let lib = read(
        b"@article{lind2020,\n  title = {Moths},\n  owner = {eva},\n}\n",
        Some(Format::Bibtex),
    );
    let out = write(&lib.references, Format::Ris).unwrap();
    assert!(out.text.contains("ID  - lind2020\n"), "{}", out.text);
    let fields: Vec<_> = out.diagnostics.iter().map(|d| d.field.as_deref()).collect();
    assert_eq!(
        fields,
        [Some("custom.source-fields")],
        "{:?}",
        out.diagnostics
    );
}
