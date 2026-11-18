//! Typst's own crates read what Pinakes writes. Every fixture is written as Hayagriva YAML and
//! as BibLaTeX; `hayagriva` and `biblatex` must parse each file, find every record under the
//! same key, and see the same number of authors. The BibTeX fixtures are also read by
//! `biblatex` directly, to compare its reader with Pinakes'.

use pinakes::{read, write, Format, Reference};
use std::fs;
use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if p.is_dir() {
            if name != "expected" {
                files(&p, out);
            }
        } else if !name.ends_with(".md") {
            out.push(p);
        }
    }
}

fn fixtures() -> Vec<(String, Vec<Reference>, Vec<u8>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let mut paths = Vec::new();
    files(&root, &mut paths);
    paths
        .into_iter()
        .map(|p| {
            let bytes = fs::read(&p).unwrap();
            let rel = p.strip_prefix(&root).unwrap().to_string_lossy().to_string();
            (rel, read(&bytes, None).references, bytes)
        })
        .filter(|(_, refs, _)| !refs.is_empty())
        .collect()
}

/// People and bodies, the way both formats count them.
fn authors(r: &Reference) -> usize {
    r.author.len()
}

fn written_keys(refs: &[Reference]) -> Vec<String> {
    let mut keyed = refs.to_vec();
    pinakes::assign_keys(&mut keyed, false);
    keyed.into_iter().map(|r| r.citation_key.unwrap()).collect()
}

#[test]
fn hayagriva_reads_our_yaml() {
    let mut checked = 0;
    for (rel, refs, _) in fixtures() {
        let yaml = write(&refs, Format::Hayagriva).unwrap().text;
        let lib = hayagriva::io::from_yaml_str(&yaml)
            .unwrap_or_else(|e| panic!("{rel}: hayagriva rejected our YAML: {e:?}\n{yaml}"));
        let keys = written_keys(&refs);
        assert_eq!(lib.len(), refs.len(), "{rel}: record count");
        for (r, key) in refs.iter().zip(&keys) {
            let entry = lib
                .get(key)
                .unwrap_or_else(|| panic!("{rel}: hayagriva has no entry {key}"));
            let theirs = entry.authors().map_or(0, |a| a.len());
            assert_eq!(theirs, authors(r), "{rel} {key}: authors");
            assert_eq!(
                entry.title().is_some(),
                r.title.is_some(),
                "{rel} {key}: title"
            );
        }
        checked += refs.len();
    }
    assert!(checked > 100, "only {checked} records checked");
}

#[test]
fn biblatex_reads_our_biblatex() {
    let mut checked = 0;
    for (rel, refs, _) in fixtures() {
        let tex = write(&refs, Format::Biblatex).unwrap().text;
        let bib = biblatex::Bibliography::parse(&tex)
            .unwrap_or_else(|e| panic!("{rel}: biblatex rejected our BibLaTeX: {e:?}\n{tex}"));
        let keys = written_keys(&refs);
        assert_eq!(bib.len(), refs.len(), "{rel}: record count");
        for (r, key) in refs.iter().zip(&keys) {
            let entry = bib
                .get(key)
                .unwrap_or_else(|| panic!("{rel}: biblatex has no entry {key}"));
            let theirs = entry.author().map_or(0, |a| a.len());
            assert_eq!(theirs, authors(r), "{rel} {key}: authors\n{tex}");
            assert_eq!(
                entry.title().is_ok(),
                r.title.is_some(),
                "{rel} {key}: title"
            );
        }
        checked += refs.len();
    }
    assert!(checked > 100, "only {checked} records checked");
}

/// Where `biblatex` and Pinakes read the same BibTeX file differently. Not an assertion of who
/// is right: the test prints the differences and fails only if `biblatex` reads a record
/// Pinakes drops.
#[test]
fn biblatex_and_pinakes_read_the_same_bibtex() {
    for (rel, refs, bytes) in fixtures() {
        if !(rel.ends_with(".bib")) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        match biblatex::Bibliography::parse(&text) {
            Ok(bib) => {
                eprintln!(
                    "{rel}: biblatex {} records, Pinakes {}",
                    bib.len(),
                    refs.len()
                );
                assert!(bib.len() <= refs.len(), "{rel}: biblatex read more records");
            }
            Err(e) => eprintln!(
                "{rel}: biblatex refused the file ({e:?}); Pinakes read {}",
                refs.len()
            ),
        }
    }
}
