//! What the old implementations' tests checked, checked again against Pinakes. Each section
//! names the implementation; fixtures/CONFORMANCE.md lists where Pinakes does differently and why.

use pinakes::{find_duplicates, read, read_str, write, Format, Library, Relation, Severity};
use std::path::Path;

fn fixture(rel: &str) -> Library {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel);
    read(&std::fs::read(&path).unwrap(), None)
}

fn codes(lib: &Library) -> Vec<&'static str> {
    lib.diagnostics.iter().map(|d| d.code).collect()
}

fn errors(lib: &Library) -> Vec<&'static str> {
    lib.diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.code)
        .collect()
}

// ---- kittum ----------------------------------------------------------------------------------

#[test]
fn kittum_medline() {
    let lib = fixture("kittum/pubmed_export.nbib");
    assert_eq!(lib.format, Some(Format::Medline));
    let r = &lib.references[0];
    // Wrapped TI and AB lines join with one space.
    assert!(r.title.as_deref().unwrap().contains("Rural Roads Lit"));
    assert!(!r.abstract_text.as_deref().unwrap().contains('\n'));
    // Authors from FAU/AU, then the group authors from CN.
    let literal: Vec<_> = r
        .author
        .iter()
        .filter_map(|n| n.literal.as_deref())
        .collect();
    assert_eq!(literal, ["LANTERN Survey Group", "BEACON Survey Group"]);
    // The DOI comes from the [doi] AID, never the [pii].
    assert_eq!(r.doi.as_deref(), Some("10.5555/ecola.1409547"));
    assert_eq!(r.pmcid.as_deref(), Some("PMC9000101"));
    assert_eq!(
        r.container_title.as_deref(),
        Some("Journal of nocturnal ecology")
    );
    assert!(r.publication_types.contains(&"Journal Article".to_string()));
    assert_eq!(lib.references.len(), 2);
}

#[test]
fn kittum_scopus() {
    let lib = fixture("kittum/scopus_export.csv");
    assert_eq!(lib.format, Some(Format::Csv));
    assert_eq!(lib.references[0].id, "2-s2.0-85000000101");
    assert_eq!(lib.references[0].pmid.as_deref(), Some("30000101"));
    // "[No abstract available]" is Scopus saying there is none.
    assert_eq!(lib.references[1].abstract_text, None);
}

#[test]
fn kittum_web_of_science() {
    let plain = fixture("kittum/wos_export.txt");
    assert_eq!(plain.format, Some(Format::WosText));
    let r = &plain.references[0];
    assert_eq!(r.id, "WOS:000500000100007");
    assert_eq!(
        r.author.len(),
        5,
        "AU continuation lines and the GP group authors"
    );
    assert_eq!(r.author[4].literal.as_deref(), Some("BEACON Survey Grp"));

    let tab = fixture("kittum/wos_export_tab.txt");
    let utf16 = fixture("kittum/wos_export_tab.utf16.txt");
    assert_eq!(tab.format, Some(Format::WosTab));
    assert_eq!(
        tab.references, utf16.references,
        "UTF-8 with BOM and UTF-16 read the same"
    );
    assert_eq!(
        tab.references[1].publication_types,
        ["Article", "Proceedings Paper"]
    );
}

#[test]
fn kittum_own_lists() {
    let lib = fixture("kittum/own_list.csv");
    assert_eq!(lib.references.len(), 3);
    assert_eq!(
        lib.references[0].doi.as_deref(),
        Some("10.5555/ecola.1409547")
    );
    assert_eq!(
        lib.references[0].author.len(),
        3,
        "authors separated by semicolons"
    );
    let cp1252 = fixture("kittum/own_list_cp1252.csv");
    assert_eq!(
        cp1252.references[0].title.as_deref(),
        Some("Nachtfalter an Straßenlaternen in München")
    );
    assert!(codes(&cp1252).contains(&"text.windows-1252"));
}

#[test]
fn kittum_central_and_embase() {
    let central = fixture("kittum/central_export.ris");
    assert_eq!(central.references[0].id, "CN-01077759");
    assert_eq!(
        central.references[0].pmid, None,
        "a CENTRAL accession is not a PMID"
    );
    let packed = fixture("kittum/central_no_blank_lines.ris");
    assert_eq!(packed.references.len(), 2);
    let embase = fixture("kittum/embase_export.ris");
    assert!(
        embase.references.iter().all(|r| r.pmid.is_none()),
        "an Embase AN is not a PMID"
    );
}

#[test]
fn kittum_refusals_are_diagnostics() {
    // kittum refused these inputs. Pinakes reads what it can and reports the rest.
    let cut = read_str(
        "FN Clarivate Analytics Web of Science\nVR 1.0\nPT J\nAU Lind, K\nTI Moths\nUT WOS:1\nER\n",
        None,
    );
    assert!(
        codes(&cut).contains(&"wos.missing-ef"),
        "{:?}",
        cut.diagnostics
    );

    let no_er = read_str("TY  - JOUR\nTI  - Moths\n", None);
    assert_eq!(no_er.references.len(), 1);
    assert!(codes(&no_er).contains(&"ris.missing-er"));

    let ris_as_wos = read_str("TY  - JOUR\nTI  - Moths\nER  - \n", Some(Format::WosText));
    assert!(ris_as_wos.has_errors());
    let ris_as_medline = read_str("TY  - JOUR\nTI  - Moths\nER  - \n", Some(Format::Medline));
    assert!(ris_as_medline.has_errors());

    let no_title_column = read_str("Authors,Year\nLind K,2019\n", Some(Format::Csv));
    assert!(codes(&no_title_column).contains(&"csv.no-title-column"));

    let no_title = read_str("Title,Year\n,2019\nMoths,2020\n", Some(Format::Csv));
    assert!(codes(&no_title).contains(&"record.no-title"));

    let pmcid = fixture("hard/csv/excel.tsv");
    assert_eq!(pmcid.references[1].pmid, None);
    assert_eq!(pmcid.references[1].pmcid.as_deref(), Some("PMC123"));
    assert!(codes(&pmcid).contains(&"id.pmcid-as-pmid"));

    let xlsx = read(b"PK\x03\x04\x14\x00\x06\x00", None);
    assert_eq!(errors(&xlsx), ["format.binary"]);
}

#[test]
fn kittum_duplicates() {
    // The same survey from three databases, and its later follow-up, which is not a duplicate.
    let mut refs = fixture("kittum/pubmed_export.nbib").references;
    refs.extend(fixture("kittum/scopus_export.csv").references);
    refs.extend(fixture("kittum/wos_export.txt").references);
    let found = find_duplicates(&refs);
    let first: Vec<_> = found
        .groups
        .iter()
        .find(|g| g.members.contains(&0))
        .unwrap()
        .members
        .clone();
    assert_eq!(
        first,
        [0, 2, 4],
        "PubMed, Scopus and WoS records of the same DOI"
    );
}

// ---- nisaba ----------------------------------------------------------------------------------

#[test]
fn nisaba_ris() {
    let lib = fixture("nisaba/unknown-tags.ris");
    let r = &lib.references[0];
    assert_eq!(
        r.doi.as_deref(),
        Some("10.1000/abc"),
        "a DOI URL, in lower case"
    );
    assert_eq!(
        r.container_title.as_deref(),
        Some("Journal of Testing"),
        "T2"
    );
    assert_eq!(r.note.as_deref(), Some("database search: MEDLINE"), "N1");
    let out = write(&lib.references, Format::Ris).unwrap().text;
    assert!(
        out.contains("QZ  - vendor-required\n"),
        "unknown tags are written back:\n{out}"
    );
    assert!(out.trim_end().ends_with("ER  -"));

    let wrapped = fixture("nisaba/wrapped.ris");
    assert_eq!(
        wrapped.references[0].abstract_text.as_deref(),
        Some("First part of a long abstract continued on the wrapped line")
    );

    let truncated = fixture("nisaba/truncated.ris");
    assert_eq!(truncated.diagnostics[0].code, "ris.missing-er");
    assert_eq!(truncated.diagnostics[0].line, Some(1));

    let orphan = read(
        &std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/nisaba/orphan-continuation.ris"),
        )
        .unwrap(),
        Some(Format::Ris),
    );
    assert_eq!(
        (orphan.diagnostics[0].severity, orphan.diagnostics[0].line),
        (Severity::Error, Some(1))
    );

    let after = fixture("nisaba/after-er.ris");
    let stray = after
        .diagnostics
        .iter()
        .find(|d| d.code == "ris.stray-text")
        .unwrap();
    assert_eq!((stray.severity, stray.line), (Severity::Error, Some(3)));
}

#[test]
fn nisaba_exports_keep_order() {
    for rel in [
        "nisaba/sample.ris",
        "nisaba/endnote-export.ris",
        "nisaba/zotero-export.ris",
    ] {
        let lib = fixture(rel);
        assert_eq!(lib.references.len(), 2, "{rel}");
        assert!(!lib.has_errors(), "{rel}: {:?}", lib.diagnostics);
    }
    let zotero = fixture("nisaba/zotero-export.ris");
    assert_eq!(zotero.references[0].pmid.as_deref(), Some("34567890"));
    let authors: Vec<_> = zotero.references[0]
        .author
        .iter()
        .map(|n| n.family.as_deref().unwrap())
        .collect();
    assert_eq!(authors, ["Müller", "García"]);
}

#[test]
fn nisaba_duplicates() {
    let lib = read_str(
        "TY  - JOUR\nTI  - One\nDO  - 10.5555/A\nER  - \nTY  - JOUR\nTI  - Two\nDO  - https://doi.org/10.5555/a\nER  - \n\
         TY  - JOUR\nTI  - Three\nAN  - 30000101\nER  - \nTY  - JOUR\nTI  - Four\nAN  - 30000101\nER  - \n\
         TY  - JOUR\nTI  - A Study: of Moths!\nAU  - Lind, K\nPY  - 2020\nER  - \nTY  - JOUR\nTI  - a study of moths\nAU  - Lind, K\nPY  - 2021\nER  - \n",
        None,
    );
    let found = find_duplicates(&lib.references);
    let pairs: Vec<_> = found.pairs.iter().map(|c| (c.a, c.b, c.relation)).collect();
    assert_eq!(
        pairs,
        [
            (0, 1, Relation::Duplicate),
            (2, 3, Relation::Duplicate),
            (4, 5, Relation::Duplicate)
        ]
    );
}

#[test]
fn nisaba_hayagriva() {
    let lib = read_str(
        "TY  - JOUR\nTI  - Moths: \"quoted\"\nAU  - Lind, K\nPY  - 2020\nER  - \n",
        None,
    );
    let yaml = write(&lib.references, Format::Hayagriva).unwrap().text;
    assert!(yaml.starts_with("lind2020moths:\n"), "{yaml}");
    assert!(yaml.contains("title: \"Moths: \\\"quoted\\\"\""), "{yaml}");
}

// ---- siglum ----------------------------------------------------------------------------------

#[test]
fn siglum_bibtex() {
    let lib = fixture("siglum/library.bib");
    let [article, report] = &lib.references[..] else {
        panic!("{:?}", lib.references)
    };
    assert_eq!(
        article.doi.as_deref(),
        Some("10.5555/jne.1"),
        "a DOI URL becomes the DOI"
    );
    assert_eq!(
        article
            .issued
            .as_ref()
            .and_then(|d| d.start)
            .and_then(|y| y.month),
        Some(5),
        "month = may"
    );
    assert_eq!(article.page.as_deref(), Some("301-310"));
    assert_eq!(report.kind, "report");
    assert_eq!(
        report.publisher.as_deref(),
        Some("Northwind Field Station"),
        "institution"
    );
    assert_eq!(report.number.as_deref(), Some("XY-123"));

    let names = fixture("siglum/names.bib");
    let first = &names.references[0].author;
    assert_eq!(first[2].particle.as_deref(), Some("van der"));
    assert_eq!(
        first[3].literal.as_deref(),
        Some("International Astronomical Society"),
        "braces keep a name whole"
    );
    assert_eq!(first.len(), 5);
    assert_eq!(
        first[4],
        pinakes::Name::others(),
        "\"and others\" is kept as the others marker"
    );
    let beethoven = &names.references[1].author[0];
    assert_eq!(
        (beethoven.particle.as_deref(), beethoven.family.as_deref()),
        (Some("van"), Some("Beethoven"))
    );
    let markup = names.references[2].title.as_deref().unwrap();
    assert!(
        markup.contains('–') && markup.contains("<i>in vitro</i>"),
        "{markup}"
    );
}

#[test]
fn siglum_ris() {
    let lib = fixture("siglum/endnote-crlf.ris");
    let [article, page] = &lib.references[..] else {
        panic!()
    };
    assert_eq!(
        article.pmid.as_deref(),
        Some("31234567"),
        "a numeric AN is a PMID"
    );
    assert_eq!(page.kind, "webpage", "ELEC");
    assert!(page.accessed.is_some(), "Y2");
    assert_eq!(
        fixture("siglum/one.txt").references[0].title.as_deref(),
        Some("T")
    );
}

#[test]
fn siglum_keys() {
    let lib = fixture("siglum/library.bib");
    let mut refs = lib.references.clone();
    pinakes::assign_keys(&mut refs, true);
    assert_eq!(refs[0].citation_key.as_deref(), Some("mueller2020moth"));
    assert_eq!(refs[1].citation_key.as_deref(), Some("northwind2023field"));
}
