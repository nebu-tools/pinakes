//! Every file under `fixtures/` (except the gitignored `generated/`) is read, and what Pinakes makes of it is compared with
//! `fixtures/expected/<path>.json`: the format it detected, every diagnostic, and the records as
//! CSL-JSON. Run with `PINAKES_BLESS=1` to rewrite the expected files after a deliberate change,
//! then read the diff.

use pinakes::{read, write, Format};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn inputs(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if p.is_dir() {
            if name != "expected" && name != "generated" {
                inputs(&p, out);
            }
        } else if !name.ends_with(".md") {
            out.push(p);
        }
    }
}

/// The format a fixture is read as: sniffed, except where the old tests named it.
fn format_for(rel: &str) -> Option<Format> {
    match rel {
        // Bare text that could be several formats; the old tests chose by file name.
        "siglum/one.bib" => Some(Format::Bibtex),
        // nisaba read this as RIS and reported the stray line; sniffing finds no RIS tag.
        "nisaba/orphan-continuation.ris" => Some(Format::Ris),
        _ => None,
    }
}

pub fn summary(rel: &str, bytes: &[u8]) -> Value {
    let lib = read(bytes, format_for(rel));
    let refs: Value =
        serde_json::from_str(&write(&lib.references, Format::CslJson).unwrap().text).unwrap();
    json!({
        "format": lib.format.map(|f| f.name()),
        "diagnostics": lib.diagnostics.iter().map(|d| json!({
            "severity": d.severity.as_str(),
            "code": d.code,
            "line": d.line,
            "record": d.record,
            "field": d.field,
            "message": d.message,
        })).collect::<Vec<_>>(),
        "references": refs,
    })
}

#[test]
fn every_fixture_matches_its_expected_reading() {
    let root = root();
    let mut files = Vec::new();
    inputs(&root, &mut files);
    assert!(files.len() > 40, "found only {} fixtures", files.len());
    let bless = std::env::var_os("PINAKES_BLESS").is_some();
    let mut failures = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let got = summary(&rel, &fs::read(&path).unwrap());
        let expected_path = root.join("expected").join(format!("{rel}.json"));
        let pretty = serde_json::to_string_pretty(&got).unwrap() + "\n";
        if bless {
            fs::create_dir_all(expected_path.parent().unwrap()).unwrap();
            fs::write(&expected_path, &pretty).unwrap();
            continue;
        }
        match fs::read_to_string(&expected_path) {
            Ok(want) if serde_json::from_str::<Value>(&want).unwrap() == got => {}
            Ok(_) => failures.push(format!(
                "{rel}: reading differs from {}",
                expected_path.display()
            )),
            Err(_) => failures.push(format!("{rel}: no expected file; run with PINAKES_BLESS=1")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
