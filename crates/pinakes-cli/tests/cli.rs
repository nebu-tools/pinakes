use std::io::Write;
use std::process::{Command, Output, Stdio};

fn pinakes(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pinakes"))
        .args(args)
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

const RIS: &str =
    "TY  - JOUR\nTI  - Moths at dusk\nAU  - Okafor, Ada\nPY  - 2019\nDO  - 10.5555/X\nER  - \n";

#[test]
fn convert_reads_stdin_and_writes_keys() {
    let out = pinakes(&["convert", "--to", "biblatex"], RIS);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let bib = text(&out.stdout);
    assert!(bib.starts_with("@article{okafor2019moths,"), "{bib}");
    assert!(bib.contains("= {10.5555/x},"), "{bib}");
}

#[test]
fn convert_joins_files_and_refuses_read_only_formats() {
    let out = pinakes(
        &[
            "convert",
            "kittum/bats_central.ris",
            "kittum/bats_embase.ris",
            "--to",
            "csl-json",
        ],
        "",
    );
    assert!(out.status.success());
    let items: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(items.as_array().unwrap().len(), 11);

    let out = pinakes(&["convert", "--to", "medline"], RIS);
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("does not write it"));
}

#[test]
fn check_reports_diagnostics_with_lines_and_fails_on_errors() {
    let out = pinakes(
        &["check"],
        "TY  - JOUR\nTI  - One\nER  - \nTY  - JOUR\nTI  - Two\n",
    );
    assert_eq!(out.status.code(), Some(0), "a missing ER is a warning");
    assert!(
        text(&out.stderr).contains("<stdin>:4: warning ris.missing-er:"),
        "{}",
        text(&out.stderr)
    );
    assert!(text(&out.stdout).contains("<stdin>: 2 records as ris; 0 errors, 1 warnings"));

    let out = pinakes(&["check", "--from", "medline"], RIS);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("error format.mismatch"));
}

#[test]
fn check_json_is_one_object_per_file() {
    let out = pinakes(
        &["check", "--json", "kittum/generic.ris", "siglum/one.bib"],
        "",
    );
    let lines: Vec<serde_json::Value> = text(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["format"], "ris");
    assert_eq!(lines[1]["file"], "siglum/one.bib");
}

#[test]
fn dedupe_finds_pairs_across_files() {
    let out = pinakes(
        &[
            "dedupe",
            "--json",
            "kittum/bats_central.ris",
            "kittum/bats_embase.ris",
        ],
        "",
    );
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["records"][6]["file"], "kittum/bats_embase.ris");
    assert_eq!(v["records"][6]["number"], 1);
    let first = &v["pairs"][0];
    assert_eq!(
        (first["a"].as_u64(), first["b"].as_u64()),
        (Some(0), Some(6))
    );
    assert_eq!(first["reasons"][0]["text"], "Same DOI 10.1000/bat.2019.001");

    let out = pinakes(
        &[
            "dedupe",
            "kittum/bats_central.ris",
            "kittum/bats_embase.ris",
        ],
        "",
    );
    let shown = text(&out.stdout);
    assert!(
        shown.starts_with(
            "duplicate 100: Same DOI 10.1000/bat.2019.001\n  kittum/bats_central.ris#1  "
        ),
        "{shown}"
    );
}
