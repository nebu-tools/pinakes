# Pinakes

Pinakes reads bibliographic references from the files that reference managers and literature
databases export. It also writes them in other formats, puts their identifiers in one form,
gives them citation keys, and finds records that may be the same publication.

It is a Rust library with a command-line tool, a WebAssembly package for JavaScript and
TypeScript, and a Python module. All four share one reader for each format. A reference is a
CSL-JSON item in every language, and fields that Pinakes does not know are kept.

The readers do not stop at bad input and do not discard it silently. Each problem becomes a
diagnostic with a code, a message and a line number, and reading continues. Writers report
in the same way: each value that the output format cannot hold as it was, and so leaves out
or changes, becomes a diagnostic that names the record and the field. Pinakes reads
UTF-8, UTF-16 and Windows-1252 (Latin-1) text, with or without a byte-order mark, and with
any line endings.

Pinakes does not format citations. To print a bibliography in a citation style, give the
CSL-JSON or Hayagriva output to a CSL processor or to Typst.

[DESIGN.md](DESIGN.md) describes the model, the entry points and the diagnostics.
[fixtures/CONFORMANCE.md](fixtures/CONFORMANCE.md) compares Pinakes with the readers it
replaces in nisaba, siglum and kittum.

## Formats

| Format | Read | Write |
| --- | --- | --- |
| RIS: EndNote, Zotero, Mendeley, PubMed, Ovid and Embase, Web of Science, Scopus, Cochrane | yes | yes |
| BibTeX | yes | yes |
| BibLaTeX | yes | yes |
| CSL-JSON | yes | yes |
| Hayagriva YAML (Typst) | no | yes |
| PubMed MEDLINE (`.nbib`) | yes | no |
| PubMed XML | yes | no |
| Web of Science plain text and tab-delimited | yes | no |
| CSV and TSV, such as Scopus exports | yes | no |

Pinakes recognises the format from the content of the file. It does not read EndNote XML
or Excel workbooks.

It normalises DOIs, PMIDs, PMCIDs, ISBNs, ISSNs and arXiv IDs. It checks the check digits of
ISBNs and ISSNs.

## Install

Pinakes is not yet on crates.io, npm or PyPI. Install it from git.

Rust library, in `Cargo.toml`:

```toml
[dependencies]
pinakes = { git = "https://github.com/nebu-tools/pinakes" }
```

Command-line tool:

```sh
cargo install --git https://github.com/nebu-tools/pinakes pinakes-cli
```

Python 3.9 or later. This builds the module from source, so it needs a Rust toolchain:

```sh
pip install "pinakes @ git+https://github.com/nebu-tools/pinakes#subdirectory=bindings/python"
```

JavaScript and TypeScript. Build the WebAssembly module, then install the package from the
checkout. The build needs the `wasm32-unknown-unknown` target and `wasm-bindgen-cli` 0.2.130:

```sh
git clone https://github.com/nebu-tools/pinakes
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130
sh pinakes/bindings/wasm/build.sh
npm install ./pinakes/bindings/wasm
```

## Examples

Each example reads `refs.ris`, writes the records in another format, and prints the
duplicates that it finds.

### Shell

```sh
pinakes convert refs.ris --to biblatex -o refs.bib
pinakes convert refs.ris other.bib --to csl-json --keys
pinakes check refs.ris
pinakes dedupe refs.ris other.bib
```

`check` prints each diagnostic with its file and line, and exits with status 1 if any is an
error. For a file with a line outside any record, it prints:

```text
bad.ris:4: error ris.stray-text: Text outside any record was not read: "this line belongs to nothing"
bad.ris: 1 record as ris; 1 error, 0 warnings
```

`check` prints only errors unless you add `--verbose`.

`convert` prints on standard error what the output format could not hold, such as a BibTeX
`owner` field written to RIS:

```text
<stdout>: warning ris.left-out: Record 1 (lind2020): ris has no place for custom.source-fields; left out [{"format":"bibtex","name":"owner","value":"eva"}]
```

Add `--strict` to make any of these a failure exit.

Add `--json` to `check` or `dedupe` to get JSON instead of text.

### Rust

```rust
use pinakes::{find_duplicates, read, write, Format};

let bytes = std::fs::read("refs.ris")?;
let lib = read(&bytes, None);
for d in &lib.diagnostics {
    eprintln!("line {:?}: {}", d.line, d.message);
}
print!("{}", write(&lib.references, Format::CslJson)?.text);
for pair in find_duplicates(&lib.references).pairs {
    let why: Vec<String> = pair.reasons.iter().map(|r| r.describe()).collect();
    println!("{} and {}: {}", pair.a, pair.b, why.join("; "));
}
```

### JavaScript

```js
import { readFileSync } from "node:fs";
import { read, write, findDuplicates } from "pinakes";

const lib = read(readFileSync("refs.ris"));
for (const d of lib.diagnostics) console.error(`line ${d.line}: ${d.message}`);
console.log(write(lib.references, "biblatex").text);
for (const pair of findDuplicates(lib.references).pairs) {
  console.log(pair.a, pair.b, pair.reasons.map((r) => r.text).join("; "));
}
```

The `pinakes` entry point loads the module from disk, for Node, Bun and Deno. In a browser,
import `pinakes/web` and wait for it to fetch the module:

```js
import load from "pinakes/web";
const { read } = await load();
```

### Python

```python
import pathlib
import pinakes

lib = pinakes.read(pathlib.Path("refs.ris").read_bytes())
for d in lib["diagnostics"]:
    print(f"line {d['line']}: {d['message']}")
print(pinakes.write(lib["references"], "hayagriva")["text"])
for pair in pinakes.find_duplicates(lib["references"])["pairs"]:
    print(pair["a"], pair["b"], "; ".join(r["text"] for r in pair["reasons"]))
```

### Citation keys

A key is the first author's family name, the year and the first significant word of the
title: `adeyemi2019moths`. If two records would get the same key, the second gets
`adeyemi2019mothsa`. Pinakes keeps a key that a record already has. To replace every key,
use `assign_keys(refs, true)` in Rust, `assignKeys(refs, { replace: true })` in JavaScript,
`assign_keys(refs, replace=True)` in Python, or `--rekey` with the command-line tool. To use a
key of your own, set the record's `citation-key`.

### Duplicates

`find_duplicates` returns pairs of records that may be the same publication. Each pair
comes with its reasons, for example "Same DOI 10.5555/x" or "Titles 96% alike; same year
2019". It also returns the groups that these pairs make. Pinakes does not merge the
records. You choose which record to keep.

The pairs are exactly those that comparing every pair of records would give, at any size.
The benchmark's 100,000 records take under a second, and a million take about five seconds. Each pair has a relation.
`duplicate` pairs make the groups. A preprint and its journal version (`version`), a
correction or retraction and its article (`notice`), a comment and its article (`comment`), a
chapter and its book (`part`) and one work in two languages (`translation`) are reported, but
never grouped.

## Build and test

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

WebAssembly and JavaScript (needs Node, and Bun for `bun test`):

```sh
sh bindings/wasm/build.sh
cd bindings/wasm && node --test test/*.test.js && bun test && npx -y -p typescript@5.9.3 tsc -p .
```

Python (needs maturin and pytest):

```sh
maturin build --release --manifest-path bindings/python/Cargo.toml --out dist
pip install dist/*.whl
pytest bindings/python/tests
```

Benchmark: this writes 100,000 generated RIS records to `fixtures/generated/bench.ris`, then
times reading them and finding the duplicates. Give a number of records and a path to change
them, and add `adversarial` for records built to defeat duplicate finding:

```sh
cargo run --release -p pinakes --example bench
cargo run --release -p pinakes --example bench -- 1000000 fixtures/generated/big.ris adversarial
```

Fuzzing (needs nightly Rust and cargo-fuzz). There is a target for each reader (`read_ris`,
`read_bibtex`, `read_medline`, `read_pubmed_xml`, `read_csl_json`, `read_wos`, `read_wos_tab`,
`read_csv`) and `round_trip`:

```sh
cd fuzz && cargo +nightly fuzz run read_ris -- -max_total_time=300
```

To update the expected readings in `fixtures/expected/` after a deliberate change, run
`PINAKES_BLESS=1 cargo test -p pinakes --test fixtures`, then review the diff.

## Licence

Pinakes is free software under the GNU Affero General Public License, version 3 only. See
[LICENSE](LICENSE).
