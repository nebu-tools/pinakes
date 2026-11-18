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
| `fuzz` | cargo-fuzz targets: one per reader, and one that writes what it read and reads it again. A separate workspace on nightly Rust. |

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

Writers lose only what the target format cannot hold, and no value breaks a writer's framing.
`crates/pinakes/tests/writers.rs` checks both with proptest. Any reference, with any text in
any field (braces, `@`, `ER  - ` lines, backslashes, control characters), writes as exactly
one record that reads back without an error, and Hayagriva's own parser accepts the YAML. A
reference in normal form (as `normalise` leaves it) reads back with every text field the
format holds unchanged, except for these, which the formats cannot hold:

| Format | Not held |
| --- | --- |
| RIS | Name parts. A name is one line, `Family, Given, Suffix`, and the reader reads it as exports write names (Vancouver initials, a trailing `;`). Every name survives, and a second pass gives the same names; a name of nothing but commas and semicolons is lost. A genre of up to eight digits is read back from `M3` as a PMID, as PubMed-derived exports mean it. |
| BibTeX | A thesis with no genre: `@phdthesis` implies "PhD thesis". A particle is told from the family name only by its lower case. |
| BibLaTeX | A particle, as in BibTeX. |
| Hayagriva | Markup in titles and abstracts, and fields Hayagriva has no key for. |
| CSL-JSON | Nothing. Any CSL-shaped object, with unknown members in the item, its names and its dates, reads and writes back as it came. |

BibTeX holds line and paragraph breaks as `\newline` and `\par`. Through CSL-JSON every
record from every fixture comes back unchanged, unknown fields included
(`crates/pinakes/tests/robustness.rs`).

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

BibLaTeX entries inherit fields from others, and the BibTeX reader resolves this as biber
does before it makes references:

- `xdata` copies every field of the named `@xdata` entries that the entry does not have. It
  is resolved first, through chains of `xdata`. The `@xdata` entries themselves are not
  records and are dropped after resolving.
- `crossref` copies the parent's fields that the child does not have, in order of field
  name, with BibLaTeX's mappings: a `@proceedings` title becomes an `@inproceedings`
  `booktitle`, an `@mvbook` title a `maintitle`, a `@periodical` title a `journaltitle`, and
  a `@book` author also a `bookauthor`. Fields such as `ids`, `crossref`, `label` and
  `shorthand` are never inherited.
- `xref` names a parent and inherits nothing.
- A field the entry has is never overwritten. A parent that is not in the file gives
  `bibtex.missing-parent`. One found only with different case gives `bibtex.parent-case` and
  is used. An `xdata` that names an entry of another type gives `bibtex.xdata-not-xdata`. A
  chain that leads back to an entry gives the error `bibtex.inheritance-cycle`, and that link
  is not followed. Resolution walks the chains with its own stack, so a chain of any length
  cannot overflow the call stack.

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

Only identifiers that the `id` module accepts count. A PMID "bad123" is no PMID.

A candidate is a `duplicate` only when nothing says the two records are different
publications. Five other relations are candidates too, so the caller can see them, but they
never join a group:

| Relation | The pair |
| --- | --- |
| `version` | A preprint and its journal version. |
| `notice` | A correction, erratum, retraction or expression of concern and the record it is about. |
| `comment` | A comment, letter or reply and the article it answers. |
| `part` | A chapter or conference paper and the book or proceedings that holds it. |
| `translation` | The same work in two languages: both records give a language, and the languages differ. |

"Part I" and "Part II" of one title are not a candidate at all: part numbers are tokens that
tell titles apart, as phases and study numbers are. Groups never hold two different DOIs,
PMIDs or PMCIDs.

#### What is compared

The result is exactly the one that comparing every pair gives, for any number of records, and
it does not depend on the order of the input. kittum compares every pair, which for 100,000
records means 5 × 10⁹ comparisons. Pinakes generates a set of pairs that provably holds every
pair the rules can accept, and compares only those:

- every pair that shares an identifier;
- of the rest, only pairs that rule 1 leaves to the titles (no identifier kind on both
  records), and that rule 2 allows: the same first author or one missing, and years at most
  one apart or one missing. Records are put in buckets by first author and year, and only
  buckets that can agree are joined;
- within the joined buckets, only pairs whose titles can reach the score the pair needs: 90
  where the first author or year can agree, 98 where neither can. A token-sort ratio of `t`
  needs `2·min(la, lb) ≥ t/100·(la + lb)` for title lengths `la` and `lb`. And because the
  longest common subsequence holds no more of any character than either title does, it needs
  the L1 distance between the titles' character counts to be at most
  `(1 − t/100)·(la + lb)`.

Each condition follows from the rules or from the score's definition, so no pair that the
rules accept is skipped. Two tests compare the result with the exhaustive comparison:
`candidates_are_what_exhaustive_finds` on 2,300 records built to defeat blocking (random
titles from a small vocabulary, and copies with a typo, a word dropped or moved, a change of
case or punctuation, the year one off, or no author), and the proptest
`generated_pairs_lose_nothing_and_order_does_not_matter`, which also shuffles the input.
Duplicates are joined into groups in an order fixed by the records' content (identifier
matches first, then by score, then by content), so a permutation of the input permutes the
result and changes nothing else.

The work is spread over the machine's cores with scoped threads, except in WebAssembly.

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

These check it:

- `crates/pinakes/tests/robustness.rs` uses proptest. It gives every reader random bytes,
  text shaped like each format, and fixtures cut short or with a byte changed, and checks that
  no reader panics.
- `fuzz/` holds cargo-fuzz targets: one for each reader, and `round_trip`, which reads with
  any reader and checks that every writer's output reads back as the same number of records.
  Run one with `cargo +nightly fuzz run read_ris` from `fuzz/`.
- `crates/pinakes/tests/depth.rs` gives every reader input nested 50,000 deep (braces, CSL
  HTML, LaTeX commands, XML elements, JSON arrays) and every writer a record nested 10,000
  deep, on a thread with a 1 MiB stack, so recursion per level of nesting would overflow.
- `fixtures/expected/` holds the full diagnostics for every fixture, so a change in what is
  reported shows in review.

Nesting is bounded everywhere. XML elements nest at most 256 deep; deeper tags are dropped
and their text kept, with `xml.too-deep`. JSON nests at most 128 deep, serde_json's limit,
and a deeper file is the error `csl.bad-json`. LaTeX command arguments are converted at most
64 deep, and deeper ones are kept as their text. Each level copies its argument once, so the
time is at most 64 times the length. Everything else is read with loops, not recursion.

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

`crates/pinakes/examples/bench.rs` writes a RIS file of generated records in three dialects.
One record in twenty is a copy of an earlier one. Half of the copies keep the DOI. The other
half have no identifier, so only their titles can match. The example then times `read` and
`find_duplicates`, and reports the peak memory. `bench N` sets the number of records, and
`bench N path adversarial` adds 20,000 records chosen to defeat candidate generation: 10,000
titled "Editorial", by the same few authors over the same years, a tenth without an author or
year; and 10,000 conference abstracts whose titles differ only in an abstract number, a week
count and one word, so every pair scores above 90 and the rules must turn most of them away.
`bench/kittum.py` times kittum on the same file.

| Records | Read | Find duplicates | Candidate pairs | Groups | Peak memory, read / after duplicates |
| --- | --- | --- | --- | --- | --- |
| 100,000 (35.2 MB) | 1.4 s | 0.4 s | 5,441 (1,842 by identifier) | 4,439 | 416 MB / 572 MB |
| 100,000 + 20,000 adversarial | 1.6 s | 6.0 s | 2,055,090 | 4,440 | 483 MB / 1.21 GB |
| 1,000,000 | 11.7 s | 4.7 s | 54,620 (18,014 by identifier) | 44,730 | 4.14 GB / 5.18 GB |
| 1,000,000 + 20,000 adversarial | 11.6 s | 9.5 s | 2,105,463 | 44,731 | 4.21 GB / 5.54 GB |

Identical "Editorial" titles by the same author within a year of each other are candidates
of each other by the rules, so the adversarial runs report about two million pairs. That is
the answer that comparing every pair gives, and computing it costs what listing it costs. Before candidate
generation was made exact, finding duplicates took 6.2 to 6.8 s for 100,000 records and 53 s
for 1,000,000.

kittum compares every pair of records, at about 80 to 210 µs a pair. It reads 100,000
records in 6.6 s, and finds the duplicates among the first 500 in 11 to 22 s and among the
first 1,000 in 42 s. For 100,000 records it would take days. On the first 500 records the
groups that Pinakes and kittum find have the same members.

The machine was a 12-core x86-64 Linux workstation with a load average of 9 to 17 from other
work, so times vary between runs. The times are of the release build.

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
