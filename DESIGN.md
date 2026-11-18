# Pinakes design

Pinakes is one library for bibliographic references. It reads exports from reference managers
and databases, puts identifiers in normal form, gives records citation keys, writes the records
in another format, and proposes which records are the same publication. It replaces three
separate readers: nisaba's `nisaba-references` crate and its TypeScript RIS tools, siglum's
`library.js`, and kittum's Python importer and duplicate finder.
[`fixtures/CONFORMANCE.md`](fixtures/CONFORMANCE.md) lists where Pinakes behaves differently
from each of them, and why.

## Layout

| Path | What it is |
| --- | --- |
| `crates/pinakes` | The library. It does no I/O: it takes bytes or text and returns values. |
| `crates/pinakes-cli` | The `pinakes` command: `convert`, `check` and `dedupe`. It reads and writes the files. |
| `bindings/wasm` | A WebAssembly module with a JavaScript package and TypeScript types, for Node, Bun and browsers. |
| `bindings/python` | A Python extension module built with PyO3 and maturin. |
| `fixtures` | Every input the old implementations tested, cleaned, plus hand-written hard cases, and what Pinakes reads from each. |
| `bench` | The script that times kittum on the benchmark file. The benchmark itself is `crates/pinakes/examples/bench.rs`. |

The bindings are thin. Each one passes CSL-JSON text into the library and JSON text out, and
the host language parses it. A reference therefore has the same shape in every language: it is
a CSL-JSON item. Diagnostics and duplicate candidates have one JSON form, defined in
`crates/pinakes/src/json.rs`, and the CLI prints that form too.

## The model

A `Reference` is a CSL-JSON item. CSL-JSON was chosen because citation processors read it,
Zotero and Pandoc write it, and its field names have one documented meaning.

- The fields that Pinakes reads, writes or compares have typed slots: `title`, `author`,
  `issued`, `container_title`, `doi`, and so on. Names are CSL names (family, given,
  particles, suffix, or a literal). Dates are CSL date parts, with a range, `circa`, a literal
  ("in press"), and the raw text when parsing lost something ("Summer 2020").
- Pinakes' own additions, which CSL has no variable for, go under CSL's `custom` object:
  arXiv ID, accession number, database name, publication types and source fields.
- `extra` keeps every CSL-JSON field that has no typed slot, exactly as it was read. A
  CSL-JSON file read and written again keeps fields that Pinakes does not know.
- `source_fields` keeps fields from the source format that have no CSL equivalent, such as an
  RIS `M1` or a BibTeX `owner`, in source order and with repeats. A writer for the same format
  writes them back. Other writers drop them, because no other format has a place for them.

Writers lose only what the target format cannot hold. BibTeX has no paragraph break, so the
BibTeX writer joins an abstract's paragraphs. RIS has no particle field, so "van der Berg"
goes into the family name. The round-trip test in `crates/pinakes/tests/robustness.rs` reads
every fixture and writes it again. Through CSL-JSON every record comes back unchanged, unknown
fields included. Through RIS, BibTeX and BibLaTeX the title, authors, year, DOI, container,
volume and pages come back unchanged, and writing the result a second time gives the same
text.

## Entry points

The library has five calls. The same five are in JavaScript and Python, with each language's
naming.

```rust
pub fn read(bytes: &[u8], format: Option<Format>) -> Library;
pub fn write(refs: &[Reference], format: Format) -> Result<String, ReadOnlyFormat>;
pub fn normalise(r: &mut Reference) -> Vec<Diagnostic>;
pub fn assign_keys(refs: &mut [Reference], replace: bool);
pub fn find_duplicates(refs: &[Reference]) -> Duplicates;
```

`read_str` is `read` for text that is already decoded. `key_for` gives the key one reference
would get. The `id` module checks and normalises single identifiers.

### Read

`read` takes the bytes of a file and returns a `Library`: the references, the diagnostics, and
the format it read.

1. **Binary input.** A ZIP archive (such as an Excel workbook) or a PDF gives the error
   `format.binary`, and nothing is read.
2. **Decoding.** The bytes are decoded as UTF-8, as UTF-16 with or without a byte-order mark,
   or as Windows-1252 when they are not valid UTF-8. Windows-1252 is a superset of the
   printable Latin-1 characters. In a file that is mostly UTF-8, each byte that is not valid
   UTF-8 is read as Windows-1252, with the warning `text.mixed-encoding`. A BOM is removed,
   CRLF and CR become LF, and NUL characters are removed with a diagnostic.
3. **Format.** If the caller does not name a format, Pinakes recognises it from the content,
   never from a file name. If the caller names a format and the content is plainly another
   one, `read` gives `format.mismatch` and reads nothing. For example, RIS read as Web of
   Science would turn every `- ` into a title.
4. **Reading.** The format's reader makes references and diagnostics.
5. **Normalising.** Every reference goes through `normalise`.

| Format | Read | Write | Notes |
| --- | --- | --- | --- |
| RIS | yes | yes | EndNote, Zotero, Mendeley, PubMed, Ovid and Embase, Web of Science, Scopus and Cochrane dialects |
| BibTeX | yes | yes | ASCII output with LaTeX escapes |
| BibLaTeX | yes | yes | Same reader as BibTeX; Unicode output with BibLaTeX fields |
| PubMed MEDLINE (`.nbib`) | yes | no | |
| PubMed XML (EFetch) | yes | no | |
| CSL-JSON | yes | yes | A list, `{"items": [...]}`, or one object |
| Web of Science plain text | yes | no | |
| Web of Science tab-delimited | yes | no | UTF-8 or UTF-16 |
| CSV and TSV | yes | no | Scopus exports and hand-kept lists with a title column |
| Hayagriva YAML | no | yes | For Typst |

EndNote XML is not read. RIS dialects are not separate formats. One reader takes every
dialect's tags (`T1` and `TI`, `A1` and `AU`, `JF`, `JO` and `T2`, `Y1` and `PY`, `DP` as the
database provider) and its habits: dates like `2019///`, DOIs given as URLs, `M3` holding a
PMID, wrapped lines, lower-case tags, a missing final `ER`. The dialect hard cases are in
`fixtures/hard/ris/`.

### Write

`write` returns the text of a file. A format that Pinakes reads but does not write returns
`ReadOnlyFormat`. A record without a citation key gets one from `key_for` in the formats that
need keys (BibTeX, BibLaTeX and Hayagriva). The record itself is not changed.

### Normalise

`normalise` puts a reference into the form that the rest of Pinakes expects, and returns
diagnostics for what it could not use. `read` has already called it. It is public for
references that the caller makes or edits.

| Identifier | Normal form | Accepts |
| --- | --- | --- |
| DOI | `10.5555/abc`, lower case | `doi:` prefixes, `https://doi.org/` and `dx.doi.org` URLs, trailing punctuation |
| PMID | digits | `PMID:` prefixes; a `PMC…` value moves to PMCID with `id.pmcid-as-pmid` |
| PMCID | `PMC123` | bare digits, any case |
| ISBN | 13 digits | ISBN-10 or ISBN-13, with hyphens or spaces; the check digit must be correct |
| ISSN | `1234-5679` | with or without the hyphen; the check digit must be correct |
| arXiv | `2101.00001v2` or `hep-th/9901001` | `arXiv:` prefixes and `arxiv.org/abs/` URLs |

An identifier that cannot be read is kept in the record as it was. It gets an `id.invalid-…`
warning, and duplicate finding ignores it. Page ranges become `301-310`, from `301--310`,
`301 – 310` and similar forms.

### Key

A citation key is the first author's family name, the year, and the first significant word of
the title, folded to ASCII: `adeyemi2019moths`. "Ødegård" becomes `odegaard`. Articles,
prepositions and conjunctions in English, German, French, Spanish, Italian and Dutch are
skipped. A record with no author or editor
uses the first word of the publisher. When two records would get the same key, the second gets
`a`, the third `b`, and so on.

`assign_keys(refs, false)` gives a key only to the records that have none. It keeps a key that a
record already has, and it keeps a CSL `id` that looks like a key: an ASCII letter first, then
letters, digits and `_-:.`, at most 64 characters. `assign_keys(refs, true)` replaces every
key. To set a key yourself, set `citation_key`. Writers use it unchanged.

### Find duplicates

`find_duplicates` returns candidate pairs, each with a relation, a score and its reasons. It
also returns the groups that the pairs join. It does not merge the records, because only the
caller knows which copy to keep.

The rules are kittum's:

1. The first identifier that both records have decides, in the order DOI, PMID, PMCID. The same
   value makes a candidate. Different values mean two publications.
2. Without a shared identifier, two records are different publications in three cases: both
   titles name a study acronym with a number, a phase, a follow-up time or a study number, and
   the names differ; the years are more than one apart; or the first authors' family names
   differ.
3. Otherwise the titles are folded and their words sorted. The pair is a candidate if the
   titles score at least 90 for similarity and the year or first author agrees, or if they
   score at least 98 alone. The score is rapidfuzz's `token_sort_ratio`.

A pair where one record is a correction or retraction notice, or where one is a preprint and
the other its journal version, is a candidate with the relation `notice` or `version`. Groups
never hold two different DOIs, PMIDs or PMCIDs.

kittum compares every pair of records. That takes days for 100,000 records. Pinakes compares
every pair up to 2,000 records. Above that, it puts the records in blocks: records that share
an identifier, the whole title, one of their three rarest title words, their two rarest words
together, or first author and year. It compares every pair that shares an identifier. In every
other block, it sorts the records by title and compares each record with the 16 after it. A
test checks that this finds the same candidates as comparing every pair, on an input above the
limit: every pair that the full comparison accepts must be among the pairs compared. The
input mixes random titles from a small vocabulary with copies that have a typo, a word dropped
or moved, a change of case or punctuation, the year one off, or no author.

## Diagnostics

Readers never panic, and they never drop input without saying so. Every problem is a
`Diagnostic`:

```json
{"severity": "error", "code": "ris.stray-text", "message": "Text outside any record was not read: \"…\"", "line": 6, "record": null}
```

- **severity**: `error` means some input was not read, or not read as its writer meant: text
  outside any record, an unclosed BibTeX entry, broken JSON, a binary file. `warning` means
  the input was read but something about it is suspect: a missing `ER`, an invalid DOI, a
  record without a title. `info` notes what Pinakes changed: a removed BOM, a UTF-16 file.
- **code** is stable and names the format or stage: `text.*`, `format.*`, `ris.*`, `bibtex.*`,
  `medline.*`, `pubmed-xml.*`, `xml.*`, `csl.*`, `wos.*`, `csv.*`, `id.*` and `record.*`.
  Callers match on the code, not the message.
- **line** is the 1-based line in the decoded input, when the format has lines.
- **record** is the index of the reference that the diagnostic belongs to, when there is one.

`Library::has_errors` tells the caller whether anything was skipped. The CLI exits with
status 1 when it was.

Two kinds of test check this:

- `crates/pinakes/tests/robustness.rs` uses proptest. It gives every reader random bytes,
  text shaped like each format, and fixtures cut short or with a byte changed, and checks that
  no reader panics.
- `fixtures/expected/` holds the full diagnostics for every fixture, so a change in what is
  reported shows in review.

## WebAssembly and Python

The WebAssembly module is built with the `wasm` profile: `opt-level = "s"`, `panic = "abort"`,
LTO, one codegen unit, and no debug information. `bindings/wasm/build.sh` runs `wasm-bindgen`,
and `wasm-opt` if it is installed. The JavaScript package has two entry points. `pinakes` loads
the module synchronously in Node and Bun. `pinakes/web` loads it with `fetch` in browsers.

The Python module is built with the stable ABI (`abi3`, Python 3.9 and later), so one wheel
serves every Python version on a platform.

| Artifact | Size |
| --- | --- |
| `pinakes_wasm_bg.wasm` | 785,595 bytes; 315,519 gzipped |
| `pinakes_wasm_bg.wasm` with `hayagriva` and `biblatex` linked in | 1,522,129 bytes; 563,084 gzipped |
| Python wheel, Linux x86-64 | 741,492 bytes |

These sizes were measured without `wasm-opt`.

## Typst's crates

Typst uses two crates for bibliographies. `biblatex` parses BibLaTeX, and `hayagriva` reads
and writes Hayagriva YAML and formats citations. We evaluated both as dependencies, and as
checks on Pinakes' output.

**As dependencies: not used.**

- `biblatex` refuses a whole file for one bad entry. On `fixtures/hard/bibtex/hard.bib` it
  stops at byte 95 with `Expected(OpeningBrace)`, where Pinakes reads all 8 entries and
  reports the bad one. Pinakes must read what it can and report the rest, so its own reader
  stays.
- `biblatex` has no line numbers in its errors, and it does not keep unknown fields in source
  order.
- `hayagriva`'s writer would only replace about 230 lines of YAML output. Its model is
  Hayagriva's, not CSL's, so it would need a second conversion.
- Linking both into the WebAssembly module makes it 1.8 times as large: from 315,519 to 563,084
  bytes gzipped. Most of the growth is citation formatting, which Pinakes leaves out.

**As checks: used.** `crates/pinakes/tests/typst.rs` writes every fixture as Hayagriva and as
BibLaTeX. It parses the output with `hayagriva` and `biblatex`, and checks that both crates
read the same number of entries, the same keys, the same number of authors and the titles. The
crates are dev-dependencies only.

## Performance

`crates/pinakes/examples/bench.rs` writes a RIS file with 100,000 records in three dialects.
One record in twenty is a copy of an earlier one. Half of the copies keep the DOI. The other
half have no identifier, so only their titles can match. The example then times `read` and
`find_duplicates`. `bench/kittum.py` times kittum on the same file.

| Task | Pinakes | kittum |
| --- | --- | --- |
| Read 10,000 records | 0.23 s | |
| Find duplicates among 10,000 | 0.68 s | |
| Read 100,000 records (35.2 MB) | 1.3 to 3.2 s | 6.6 s |
| Find duplicates among the first 500 | 0.08 s (CLI) | 11 to 22 s |
| Find duplicates among the first 1,000 | 0.14 s (CLI) | 42 s |
| Find duplicates among 100,000 | 3.7 to 13.2 s | not run |

Pinakes finds 5,441 candidate pairs in the 100,000 records, 1,842 of them by identifier, and
4,439 groups. kittum compares every pair of records, at about 80 to 210 µs a pair. For 100,000
records that is about 5 × 10⁹ pairs, which would take days. On the first 500 records the
groups that Pinakes and kittum find have the same members.

The machine was a 12-core x86-64 Linux workstation with a load average of about 21 from other
work, so times varied by up to three times between runs. Each range is the fastest and
slowest of several runs of the release build.

## Out of scope

- **Citation formatting.** Pinakes does not render citations or bibliographies in a style. Use
  a CSL processor (citeproc-js, citeproc-rs, Hayagriva) on the CSL-JSON or Hayagriva that
  Pinakes writes.
- **Merging duplicates.** Pinakes proposes candidates and reasons. The caller decides which
  record to keep.
- **Network lookups.** Pinakes never fetches metadata for a DOI or a PMID.
- **Files.** The library reads no files and writes no files. The CLI and the bindings do.
- **EndNote XML and spreadsheets.** An `.xlsx` file gives `format.binary`. Save the sheet as
  CSV.
- **Application conventions.** Pinakes does not make stable record IDs, does not parse search
  provenance from notes, and does not find study registration numbers in free text. Each
  application defines these differently.
