//! Deeply nested input reaches no reader or writer as unbounded recursion. Every test runs on a
//! thread with a small stack, so a recursive descent over the nesting would overflow it.

use pinakes::{read, write, Format, Reference};
use serde_json::{json, Value};

/// Nesting this deep in every test. With a recursive step per level, even a small frame would
/// need far more than the stack `small_stack` gives.
const N: usize = 50_000;

fn small_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

/// A value that nests braces, CSL HTML and LaTeX commands `N` deep.
fn deep_text() -> String {
    deep_text_of(N)
}

fn deep_text_of(n: usize) -> String {
    let mut s = String::new();
    s += &"{".repeat(n);
    s += &"<i>".repeat(n);
    s += &"\\textit{".repeat(n);
    s += "T";
    s += &"}".repeat(n);
    s += &"</i>".repeat(n);
    s += &"}".repeat(n);
    s
}

/// Read `text` as `format`: one record, and nothing that panics or overflows.
fn reads_one(format: Format, text: String) {
    small_stack(move || {
        let lib = read(text.as_bytes(), Some(format));
        assert_eq!(lib.references.len(), 1, "{format}: {:?}", lib.diagnostics);
        assert!(lib.references[0].title.is_some(), "{format}");
    });
}

#[test]
fn ris() {
    reads_one(
        Format::Ris,
        format!("TY  - JOUR\nTI  - {}\nER  - \n", deep_text()),
    );
}

#[test]
fn bibtex() {
    reads_one(
        Format::Bibtex,
        format!("@article{{k,title={{{}}}}}\n", deep_text()),
    );
    // Unbalanced: every group opened and none closed.
    small_stack(|| {
        let text = format!("@article{{k,title={}\n", "{\\textit{".repeat(N));
        let lib = read(text.as_bytes(), Some(Format::Bibtex));
        assert!(!lib.diagnostics.is_empty());
    });
}

#[test]
fn medline() {
    reads_one(Format::Medline, format!("PMID- 1\nTI  - {}\n", deep_text()));
}

#[test]
fn pubmed_xml() {
    let title = format!("{}T{}", "<i>".repeat(N), "</i>".repeat(N));
    let unknown = format!("{}{}", "<x>".repeat(N), "</x>".repeat(N));
    reads_one(
        Format::PubmedXml,
        format!(
            "<PubmedArticleSet><PubmedArticle><MedlineCitation><PMID>1</PMID><Article>\
             <ArticleTitle>{title}</ArticleTitle>{unknown}</Article></MedlineCitation>\
             </PubmedArticle></PubmedArticleSet>"
        ),
    );
}

#[test]
fn csl_json() {
    reads_one(
        Format::CslJson,
        json!([{"id": "a", "type": "book", "title": deep_text()}]).to_string(),
    );
    // Nesting in members Pinakes keeps, and at the top: serde_json's limit refuses it with a
    // diagnostic, without overflowing.
    small_stack(|| {
        let deep = format!("{}{}", "[".repeat(N), "]".repeat(N));
        for text in [
            format!("[{{\"id\":\"a\",\"title\":\"T\",\"note-x\":{deep}}}]"),
            deep.clone(),
        ] {
            let lib = read(text.as_bytes(), Some(Format::CslJson));
            assert!(!lib.diagnostics.is_empty());
        }
    });
}

#[test]
fn wos_text() {
    reads_one(
        Format::WosText,
        format!(
            "FN Clarivate Analytics Web of Science\nVR 1.0\nPT J\nTI {}\nER\nEF\n",
            deep_text()
        ),
    );
}

#[test]
fn wos_tab() {
    reads_one(
        Format::WosTab,
        format!("PT\tAU\tTI\nJ\tLind, K\t{}\n", deep_text()),
    );
}

#[test]
fn csv() {
    reads_one(
        Format::Csv,
        format!("Title,Year\n\"{}\",2020\n", deep_text()),
    );
}

/// A record whose title, author and kept CSL members nest deeply. Shallower than `N`: the
/// LaTeX reader converts each command's argument again up to 64 levels down, which is
/// linear but slow in a debug build, and 10,000 levels still need far more stack than
/// `small_stack` gives if anything recursed per level.
fn deep_reference() -> Reference {
    let mut r = Reference {
        id: "a".into(),
        kind: "article-journal".into(),
        title: Some(deep_text_of(10_000)),
        ..Reference::default()
    };
    // As deep as serde_json lets a reader build it.
    let mut v = json!("leaf");
    for _ in 0..120 {
        v = Value::Array(vec![v]);
    }
    r.extra.insert("x-deep".into(), v);
    r.author = vec![pinakes::Name::literal(&deep_text_of(10_000))];
    r
}

#[test]
fn writers_and_their_readers() {
    for format in Format::ALL.into_iter().filter(|f| f.can_write()) {
        small_stack(move || {
            let out = write(&[deep_reference()], format).unwrap();
            assert!(out.contains('T'), "{format}");
            if format.can_read() {
                let lib = read(out.as_bytes(), Some(format));
                assert_eq!(lib.references.len(), 1, "{format}: {:?}", lib.diagnostics);
            }
        });
    }
}
