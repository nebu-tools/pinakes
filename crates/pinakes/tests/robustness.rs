//! Readers never panic, whatever the bytes, and what Pinakes writes it reads back the same.

use pinakes::{read, write, Format, Library, Reference};
use proptest::prelude::*;
use std::path::{Path, PathBuf};

fn fixtures() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                if !p.ends_with("expected") && !p.ends_with("generated") {
                    walk(&p, out);
                }
            } else if p.extension().is_none_or(|x| x != "md") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures"),
        &mut out,
    );
    out.sort();
    out
}

/// Every record is accounted for: read, or reported.
fn check(lib: &Library) {
    for d in &lib.diagnostics {
        assert!(!d.code.is_empty() && !d.message.is_empty());
    }
}

/// Text that looks enough like each format to reach deep into its reader.
fn near_miss() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        Just("TY  - JOUR\n".to_string()),
        Just("ER  - \n".to_string()),
        Just("AU  - Lind, K\n".to_string()),
        Just("      continued\n".to_string()),
        Just("PMID- 30000101\n".to_string()),
        Just("FAU - Okafor, Ada\n".to_string()),
        Just("PT J\n".to_string()),
        Just("AU Lind, K\n   Okafor, A\n".to_string()),
        Just("ER\n".to_string()),
        Just("EF\n".to_string()),
        Just("@article{k,\n".to_string()),
        Just("title = {Moths {at} night},\n".to_string()),
        Just("author = \"Lind, K and others\" # eco,\n".to_string()),
        Just("@string{eco = {x}}\n".to_string()),
        Just("}\n".to_string()),
        Just("<PubmedArticle><MedlineCitation><PMID>1</PMID><Article><ArticleTitle>".to_string()),
        Just("</ArticleTitle></Article></MedlineCitation></PubmedArticle>".to_string()),
        Just("&#x24b8;&bogus;<i>".to_string()),
        Just("[{\"type\":\"book\",\"title\":".to_string()),
        Just("\"x\",\"issued\":{\"date-parts\":[[2020,13,40]]}}]".to_string()),
        Just("Title,Authors,Year,DOI\n".to_string()),
        Just("\"a, b\",\"Lind K; Okafor A\",2019,10.5555/x\n".to_string()),
        Just("\"unclosed\n".to_string()),
        "[ -~\\n\\t\u{e9}\u{2013}\u{feff}]{0,40}",
    ];
    prop::collection::vec(piece, 0..24).prop_map(|v| v.concat())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    #[test]
    fn random_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..600)) {
        check(&read(&bytes, None));
        for f in Format::ALL.into_iter().filter(|f| f.can_read()) {
            check(&read(&bytes, Some(f)));
        }
    }

    #[test]
    fn near_misses_never_panic(text in near_miss()) {
        let lib = read(text.as_bytes(), None);
        check(&lib);
        for f in Format::ALL.into_iter().filter(|f| f.can_read()) {
            let lib = read(text.as_bytes(), Some(f));
            check(&lib);
            // Whatever was read can be written in every format, and read back.
            for out in Format::ALL.into_iter().filter(|f| f.can_write()) {
                let written = write(&lib.references, out).unwrap();
                if out.can_read() {
                    check(&read(written.as_bytes(), Some(out)));
                }
            }
        }
    }

    #[test]
    fn mangled_fixtures_never_panic(index in 0usize..1000, cut in 0usize..100_000, flip in any::<u8>()) {
        let files = fixtures();
        let mut bytes = std::fs::read(&files[index % files.len()]).unwrap();
        let at = cut % (bytes.len() + 1);
        bytes.truncate(at.max(1).min(bytes.len()));
        if let Some(b) = bytes.get_mut(at / 2) {
            *b ^= flip;
        }
        check(&read(&bytes, None));
    }
}

/// The fields every writable format has a place for.
fn core(r: &Reference) -> impl PartialEq + std::fmt::Debug {
    (
        r.title.clone(),
        // "de la Cruz" may come back as a particle and a family name; both read alike.
        r.author
            .iter()
            .map(|n| {
                let family = n.display_family();
                let full = match &n.dropping_particle {
                    Some(d) => format!("{d} {family}"),
                    None => family,
                };
                (full, n.literal.clone())
            })
            .collect::<Vec<_>>(),
        r.year(),
        r.doi.clone(),
        r.container_title.clone(),
        r.volume.clone(),
        r.page.clone(),
    )
}

#[test]
fn round_trips() {
    for path in fixtures() {
        let lib = read(&std::fs::read(&path).unwrap(), None);
        if lib.references.is_empty() {
            continue;
        }
        let name = path.display();
        // CSL-JSON is the model's own format: nothing is lost, unknown fields included.
        let csl = read(
            write(&lib.references, Format::CslJson).unwrap().as_bytes(),
            Some(Format::CslJson),
        );
        assert_eq!(csl.references, lib.references, "{name}: CSL-JSON");
        for f in [Format::Ris, Format::Bibtex, Format::Biblatex] {
            let text = write(&lib.references, f).unwrap();
            let back = read(text.as_bytes(), Some(f));
            assert_eq!(
                back.references.len(),
                lib.references.len(),
                "{name}: {f}\n{text}"
            );
            for (a, b) in lib.references.iter().zip(&back.references) {
                assert_eq!(core(a), core(b), "{name}: {f}, record {}\n{text}", a.id);
            }
            // Writing what was read back gives the same text: the writers are stable.
            assert_eq!(
                write(&back.references, f).unwrap(),
                text,
                "{name}: {f} is not stable"
            );
        }
    }
}
