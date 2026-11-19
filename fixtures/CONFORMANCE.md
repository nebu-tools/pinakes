# Conformance with the implementations Pinakes replaces

Pinakes replaces three readers:

- nisaba's Rust `nisaba-references` crate, with its TypeScript RIS tools and tests;
- siglum's `library.js`;
- kittum's Python importer, identifier normalisation and duplicate finder in `apps/server`.

Each one's fixtures are under `fixtures/<name>/`. They have been cleaned: the subjects are now
ecology, astronomy, history and education, and the addresses are at example.org. The structure
and the awkward formatting are unchanged. `fixtures/hard/` holds new cases for each format and
dialect. `fixtures/expected/` holds what Pinakes reads from every file, checked by
`crates/pinakes/tests/fixtures.rs`. `crates/pinakes/tests/conformance.rs` restates the old
implementations' own tests against Pinakes.

Below, for each old implementation, is every case where it extracted something Pinakes does not.
Each case is either **fixed**, meaning Pinakes now does it too, or **kept**, with the reason.

## nisaba

nisaba read RIS (`RisRecord::parse`, `import_ris`, `metadata_from_ris`, `parse_person`),
removed duplicates, and wrote Hayagriva. Its TypeScript tools (`roundtrip.ts`, `ris.test.ts`)
checked round trips and field coverage.

| Case | nisaba | Pinakes |
| --- | --- | --- |
| Lower-case tags (`ty  - JOUR`) | read | **Fixed.** A lower-case tag is accepted when it is two characters followed by exactly two spaces and `-`. Anything looser would mistake prose such as `of - the` for a tag. |
| Tab between tag and dash (`TI\t- …`) | read | **Fixed.** A tab counts as a space. |
| Indented continuation lines, with or without a leading `- ` | joined | Already the same. |
| `PMID` tag, and digits in `M3` as a PMID | read | Already the same. |
| `DP` as the publication date | read as a date | **Kept.** The RIS specification defines `DP` as the database provider, and Ovid and EBSCO export it that way. Pinakes keeps it as the database name. A date in `DP` would then be wrong, so Pinakes does not guess. |
| Search provenance in `N1` (`search: …; database: …; search date: …`) | parsed into fields | **Kept.** The note is kept whole. That layout is a nisaba convention, so nisaba should parse it. |
| A record without a title | refused | **Kept.** Pinakes reads it. Editorials and data sets sometimes lack a title, and the record is better kept than lost. |
| A file without a final `ER` | refused | **Kept.** Pinakes reads the last record and gives the warning `ris.missing-er`, with its line. |
| Record ID made from `AN`, the DOI or a hash | made | **Kept.** Pinakes uses the record's own ID when it has one and numbers the rest. How to make a stable ID is nisaba's choice. |
| Typst citation extraction, numbering, full-text file names, export manifests | done | **Kept** in nisaba. None of these is about reading or writing references. |
| Field-coverage report (TypeScript) | printed | **Kept** out. It was a tool for development. The round-trip tests in `crates/pinakes/tests/fixtures.rs` cover what it checked. |
| Duplicates by title alone | matched | **Kept.** Pinakes follows kittum's rules (see below). Two equal titles alone are a candidate only at 98 or more; otherwise the first author or year must agree too. This keeps apart titles like "Editorial" or "Annual report". |

## siglum

siglum read BibTeX, BibLaTeX, RIS and CSL-JSON in `library.js`, made citation keys, and merged
items.

Pinakes maps these fields as siglum does: `J2`, `CY`, `ET`, `L2`, `Y2` as the access date, `SN`
as an ISBN for books, `journaltitle`, `shortjournal`, `series`, `urldate`, `location`,
`organization`, the `number` of a `techreport`, and CSL-JSON given as `{"items": [...]}` or as
a single object.

| Case | siglum | Pinakes |
| --- | --- | --- |
| CSL-JSON `id` as the citation key | used | **Fixed.** A key-like `id` (an ASCII letter first, then letters, digits and `_-:.`, at most 64 characters) is kept as the key. An `id` such as `12` or a Zotero URI is not key-like, so a key is made instead. |
| `A3` as an editor | read as editor | **Kept.** In RIS `A3` is the tertiary or series author. Pinakes keeps it as a source field and does not mislabel it. |
| Thesis genre | "Dissertation" | **Kept.** Pinakes writes "PhD thesis" for `phdthesis` and "Master's thesis" for `mastersthesis`. Those are the BibTeX types' meanings, and CSL styles print the genre. |
| Citation key | family name and year | **Kept.** Pinakes adds the first significant title word (`lind2020tides`), as the brief asks. Collisions get `a`, `b`, …, and a key that is already present is never changed unless the caller asks. |
| `mergeItems` | merged duplicates | **Kept** out. Pinakes finds candidates and gives the reasons. The caller merges. |

## kittum

kittum's importer read RIS from Ovid, CENTRAL, PubMed and Scopus, MEDLINE `.nbib`, PubMed XML,
Web of Science plain text and tab-delimited files, Scopus CSV, and CSV and Excel lists kept by hand. Its
duplicate finder proposed pairs from identifiers and titles.

Pinakes reads what kittum read from these: `CN`, `BTI`, `GP`, `CA`, `EA` and `UT`; the
Scopus placeholder "[No abstract available]", which is dropped; a hand-kept CSV list in cp1252; Web of
Science files in UTF-16; correction and retraction notices; and preprints (`PPR:` IDs and
preprint DOI prefixes) together with their journal versions. Its title folding, its tokens that
tell two titles apart (an acronym with a number, a phase, a follow-up time, a study number) and
its thresholds (90 with the year or first author, 98 alone) are kept. The tests in
`conformance.rs` check them case by case.

| Case | kittum | Pinakes |
| --- | --- | --- |
| Excel `.xlsx` lists | read | **Kept** out. Pinakes gives the error `format.binary`. Reading spreadsheets needs a zip and XML stack that a reference library should not carry, and a caller can save the sheet as CSV. |
| Registration numbers found in titles, abstracts and the MEDLINE `SI` field | extracted | **Kept** out. What counts as a registration number depends on the registry, so the application should find it. `SI` stays on the record as a source field, so nothing is lost. |
| Registry CSV exports | read | **Kept** out. They list studies, not publications. |
| Refusals (raising an error) | raised | **Kept** differently. Each one is a diagnostic with a line where there is one: `format.binary`, `format.mismatch`, `wos.missing-ef`, `csv.no-title-column`, `id.pmcid-as-pmid`. The caller decides whether an error stops the import. |
| Record ID from `AN`, `ID` or the DOI | made | **Kept.** As with nisaba, the record keeps its own ID, and the caller chooses a stable scheme. |
| Paragraph breaks in abstracts, written as BibTeX | kept | **Fixed.** The BibTeX writer writes a paragraph break as `\par` and a line break as `\newline`, and the reader reads them back. |
| CENTRAL database name | "Cochrane Central Register of Controlled Trials" | Already the same. The name is kept as exported. |
| Comparing every pair | every pair | **Fixed** in what it finds, not in how. Pinakes finds exactly the pairs that comparing every pair finds, at any size, without comparing every pair: it compares only pairs that the rules could accept, and skips the others by conditions each rule needs. `candidates_are_what_exhaustive_finds` and `generated_pairs_lose_nothing_and_order_does_not_matter` check this. See `DESIGN.md`. |
| Related records that are not the same publication | `same_record`, `same_report` and `same_study` proposals for a person to confirm | **Kept** differently. Pinakes leaves reports and studies to the application. A notice and its article, or a preprint and its journal version, are candidates with the relation `notice` or `version`, and so are a comment or reply and its article (`comment`), a chapter and its book (`part`), and one work in two languages (`translation`). Only `duplicate` pairs join groups, so a retraction is never merged into the paper it retracts. |
