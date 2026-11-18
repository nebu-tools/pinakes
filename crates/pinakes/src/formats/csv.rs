//! Tables of records: Scopus CSV exports and the lists people keep in spreadsheets.
//!
//! The first row that has a title column is the header; column names are matched without case,
//! spaces or punctuation against the aliases below. Commas, semicolons (Excel in many locales)
//! and tabs all work as delimiters; the header row decides. Columns Pinakes does not know are
//! kept as source fields under their header.

use crate::dates::parse_date;
use crate::diag::Diagnostic;
use crate::model::{Name, Reference};
use crate::names::{parse_name, split_name_list};
use crate::normalise::finish;

pub const FORMAT: &str = "csv";

/// One row: its cells and the line it starts on.
pub(crate) struct Row {
    pub cells: Vec<String>,
    pub line: usize,
}

/// Split delimited text into rows. With `quotes`, RFC 4180 quoting applies: a field in double
/// quotes may hold the delimiter, line breaks and doubled quotes. Empty rows are dropped.
pub(crate) fn rows(text: &str, delim: char, quotes: bool, diags: &mut Vec<Diagnostic>) -> Vec<Row> {
    let mut out = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut line = 1;
    let mut row_line = 1;
    let mut chars = text.chars().peekable();
    let mut in_quotes = false;
    let mut quote_line = 0;
    let mut at_cell_start = true;
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    cell.push('"');
                }
                '"' => in_quotes = false,
                '\n' => {
                    line += 1;
                    cell.push('\n');
                }
                c => cell.push(c),
            }
            continue;
        }
        match c {
            '"' if quotes && at_cell_start => {
                in_quotes = true;
                quote_line = line;
                at_cell_start = false;
            }
            c if c == delim => {
                row.push(std::mem::take(&mut cell));
                at_cell_start = true;
            }
            '\n' => {
                row.push(std::mem::take(&mut cell));
                if row.iter().any(|c| !c.trim().is_empty()) {
                    out.push(Row {
                        cells: std::mem::take(&mut row),
                        line: row_line,
                    });
                } else {
                    row.clear();
                }
                line += 1;
                row_line = line;
                at_cell_start = true;
            }
            c => {
                cell.push(c);
                if !c.is_whitespace() {
                    at_cell_start = false;
                }
            }
        }
    }
    if in_quotes {
        diags.push(
            Diagnostic::error("csv.unclosed-quote", format!("A quoted field from line {quote_line} is never closed; read the rest of the file as that field"))
                .at(quote_line),
        );
    }
    row.push(cell);
    if row.iter().any(|c| !c.trim().is_empty()) {
        out.push(Row {
            cells: row,
            line: row_line,
        });
    }
    out
}

/// The delimiter of a header line: whichever of `,`, `;` and tab it has most of, outside quotes.
pub(crate) fn sniff_delimiter(first_line: &str) -> char {
    let mut counts = [0usize; 3];
    let mut q = false;
    for c in first_line.chars() {
        match c {
            '"' => q = !q,
            ',' if !q => counts[0] += 1,
            ';' if !q => counts[1] += 1,
            '\t' if !q => counts[2] += 1,
            _ => {}
        }
    }
    let best = (0..3).max_by_key(|&i| (counts[i], 3 - i)).unwrap_or(0);
    [',', ';', '\t'][best]
}

fn key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Col {
    Title,
    Abstract,
    Doi,
    Pmid,
    Pmcid,
    Authors,
    AuthorFull,
    Editors,
    Journal,
    JournalShort,
    Year,
    Date,
    Types,
    Id,
    Volume,
    Issue,
    Pages,
    PageStart,
    PageEnd,
    ArticleNumber,
    Isbn,
    Issn,
    Url,
    Keywords,
    IndexKeywords,
    Publisher,
    Place,
    Language,
    Note,
    Database,
    Edition,
}

const ALIASES: &[(Col, &[&str])] = &[
    (
        Col::Title,
        &[
            "title",
            "articletitle",
            "documenttitle",
            "primarytitle",
            "ti",
        ],
    ),
    (Col::Abstract, &["abstract", "ab", "summary"]),
    (Col::Doi, &["doi", "di", "digitalobjectidentifier"]),
    (Col::Pmid, &["pmid", "pubmedid", "pubmed", "pm"]),
    (Col::Pmcid, &["pmcid", "pmc", "pubmedcentralid"]),
    (Col::AuthorFull, &["authorfullnames", "af"]),
    (
        Col::Authors,
        &[
            "authors",
            "author",
            "authorss",
            "au",
            "authornames",
            "creator",
            "creators",
        ],
    ),
    (Col::Editors, &["editors", "editor", "ed"]),
    (
        Col::Journal,
        &[
            "journal",
            "journaltitle",
            "sourcetitle",
            "source",
            "publication",
            "publicationtitle",
            "jt",
            "so",
            "containertitle",
        ],
    ),
    (
        Col::JournalShort,
        &[
            "abbreviatedsourcetitle",
            "journalabbreviation",
            "ta",
            "ji",
            "j9",
        ],
    ),
    (
        Col::Year,
        &["year", "publicationyear", "py", "pubyear", "dp"],
    ),
    (
        Col::Date,
        &["date", "publicationdate", "issued", "datepublished"],
    ),
    (
        Col::Types,
        &[
            "publicationtype",
            "publicationtypes",
            "documenttype",
            "itemtype",
            "type",
            "pt",
            "dt",
        ],
    ),
    (
        Col::Id,
        &[
            "id",
            "recordid",
            "accessionnumber",
            "refid",
            "eid",
            "ut",
            "key",
            "citationkey",
        ],
    ),
    (Col::Volume, &["volume", "vl", "vol"]),
    (Col::Issue, &["issue", "is", "number", "no"]),
    (Col::Pages, &["pages", "page", "pg", "pagerange"]),
    (Col::PageStart, &["pagestart", "startpage", "bp", "sp"]),
    (Col::PageEnd, &["pageend", "endpage", "ep"]),
    (Col::ArticleNumber, &["artno", "articlenumber", "ar"]),
    (Col::Isbn, &["isbn", "isbns", "bn"]),
    (Col::Issn, &["issn", "sn"]),
    (Col::Url, &["url", "link", "ur"]),
    (
        Col::Keywords,
        &[
            "keywords",
            "authorkeywords",
            "kw",
            "de",
            "manualtags",
            "tags",
        ],
    ),
    (
        Col::IndexKeywords,
        &["indexkeywords", "automatictags", "meshterms", "mh"],
    ),
    (Col::Publisher, &["publisher", "pu", "pb"]),
    (
        Col::Place,
        &["place", "publisherplace", "location", "cy", "pi"],
    ),
    (
        Col::Language,
        &["language", "languageoforiginaldocument", "la", "lang"],
    ),
    (Col::Note, &["note", "notes", "n1", "extra"]),
    (Col::Database, &["database", "db", "librarycatalog"]),
    (Col::Edition, &["edition", "et"]),
];

/// Each known column's index: the first header cell that is one of its names.
fn columns(header: &[String]) -> Vec<(Col, usize)> {
    let keys: Vec<String> = header.iter().map(|h| key(h)).collect();
    let mut found = Vec::new();
    let mut taken = vec![false; keys.len()];
    for (col, names) in ALIASES {
        for name in *names {
            if let Some(i) = keys
                .iter()
                .enumerate()
                .position(|(i, k)| k == name && !taken[i])
            {
                found.push((*col, i));
                taken[i] = true;
                break;
            }
        }
    }
    found
}

fn has(cols: &[(Col, usize)], c: Col) -> bool {
    cols.iter().any(|(x, _)| *x == c)
}

/// Whether a header names a table of records Pinakes can read.
pub(crate) fn is_record_header(header: &[String]) -> bool {
    has(&columns(header), Col::Title)
}

/// "Wainwright, Claire E. (7006453478)" to "Wainwright, Claire E.".
fn strip_author_id(s: &str) -> &str {
    let t = s.trim();
    if t.ends_with(')') {
        if let Some(open) = t.rfind(" (") {
            if t[open + 2..t.len() - 1].chars().all(|c| c.is_ascii_digit()) {
                return t[..open].trim();
            }
        }
    }
    t
}

fn author_list(cell: &str) -> Vec<Name> {
    let t = cell.trim();
    if t.is_empty() || t.starts_with("[No author") {
        return Vec::new();
    }
    // Older Scopus exports separate "Wainwright C.E., Elborn J.S." with commas.
    let parts = if !t.contains(';')
        && !t.contains('\n')
        && t.split(',').count() > 1
        && t.split(',').all(|p| {
            let w: Vec<&str> = p.split_whitespace().collect();
            w.len() >= 2 && crate::names::is_initials(w[w.len() - 1])
        }) {
        t.split(',').map(|s| s.trim().to_string()).collect()
    } else {
        split_name_list(t)
    };
    parts
        .iter()
        .map(|p| parse_name(strip_author_id(p)))
        .filter(|n| !n.is_empty())
        .collect()
}

fn split_list(cell: &str) -> Vec<String> {
    cell.split([';', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn kind_from_type(t: &str) -> Option<&'static str> {
    let l = t.trim().to_lowercase();
    Some(match l.as_str() {
        "article" | "journal article" | "journalarticle" | "article-journal" | "review"
        | "letter" | "editorial" | "note" | "short survey" | "erratum" | "data paper" => {
            "article-journal"
        }
        "book" => "book",
        "book chapter" | "chapter" | "booksection" | "book section" => "chapter",
        "conference paper" | "conference review" | "conferencepaper" | "proceedings paper"
        | "paper-conference" => "paper-conference",
        "report" | "technical report" => "report",
        "thesis" | "dissertation" => "thesis",
        "web page" | "webpage" => "webpage",
        "dataset" | "data set" => "dataset",
        "preprint" => "article",
        "magazine article" => "article-magazine",
        "newspaper article" => "article-newspaper",
        _ => return None,
    })
}

/// Read a table of records.
pub fn read(text: &str, diags: &mut Vec<Diagnostic>) -> Vec<Reference> {
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let delim = sniff_delimiter(first);
    let rows = rows(text, delim, true, diags);
    if rows.is_empty() {
        if !text.trim().is_empty() {
            diags.push(Diagnostic::error(
                "csv.no-header",
                "The file has no header row",
            ));
        }
        return Vec::new();
    }
    let Some(start) = rows
        .iter()
        .take(10)
        .position(|r| is_record_header(&r.cells))
    else {
        let named: Vec<&str> = rows[0]
            .cells
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        diags.push(
            Diagnostic::error(
                "csv.no-title-column",
                format!(
                    "A table of records needs a Title column; the first row has {}",
                    named.join(", ")
                ),
            )
            .at(rows[0].line),
        );
        return Vec::new();
    };
    for skipped in &rows[..start] {
        diags.push(
            Diagnostic::info("csv.skipped-row", "Skipped a row above the header").at(skipped.line),
        );
    }
    let header = &rows[start].cells;
    let cols = columns(header);
    let known: Vec<usize> = cols.iter().map(|(_, i)| *i).collect();
    let mut out = Vec::new();
    for (index, row) in rows[start + 1..].iter().enumerate() {
        if row.cells.len() > header.len()
            && row.cells[header.len()..]
                .iter()
                .any(|c| !c.trim().is_empty())
        {
            diags.push(
                Diagnostic::warning(
                    "csv.extra-cells",
                    format!(
                        "Row has {} cells and the header {}; kept the extra cells as source fields",
                        row.cells.len(),
                        header.len()
                    ),
                )
                .at(row.line)
                .record(index),
            );
        } else if row.cells.len() < header.len() && row.cells.len() > 1 {
            diags.push(
                Diagnostic::info(
                    "csv.short-row",
                    format!(
                        "Row has {} cells and the header {}",
                        row.cells.len(),
                        header.len()
                    ),
                )
                .at(row.line)
                .record(index),
            );
        }
        let cell = |c: Col| -> Option<String> {
            let i = cols.iter().find(|(x, _)| *x == c)?.1;
            let v = row.cells.get(i)?.trim();
            (!v.is_empty()).then(|| v.to_string())
        };
        let mut r = Reference::default();
        r.title = cell(Col::Title);
        r.abstract_text = cell(Col::Abstract).filter(|a| a != "[No abstract available]");
        r.doi = cell(Col::Doi);
        r.pmid = cell(Col::Pmid);
        r.pmcid = cell(Col::Pmcid);
        r.author = match (cell(Col::AuthorFull), cell(Col::Authors)) {
            (Some(full), short) => {
                let names = author_list(&full);
                if let Some(s) = short {
                    r.keep(FORMAT, "Authors", &s);
                }
                names
            }
            (None, Some(s)) => author_list(&s),
            _ => Vec::new(),
        };
        r.editor = cell(Col::Editors)
            .map(|e| author_list(&e))
            .unwrap_or_default();
        r.container_title = cell(Col::Journal);
        r.container_title_short = cell(Col::JournalShort);
        r.issued = cell(Col::Date)
            .and_then(|d| parse_date(&d))
            .or_else(|| cell(Col::Year).and_then(|y| parse_date(y.trim_end_matches(".0"))));
        if let Some(t) = cell(Col::Types) {
            r.publication_types = split_list(&t);
            r.kind = r
                .publication_types
                .iter()
                .find_map(|t| kind_from_type(t))
                .unwrap_or("article-journal")
                .to_string();
        } else {
            r.kind = if r.container_title.is_some() {
                "article-journal"
            } else {
                "document"
            }
            .to_string();
        }
        let id = cell(Col::Id);
        r.volume = cell(Col::Volume);
        r.issue = cell(Col::Issue);
        r.page = cell(Col::Pages).or_else(|| match (cell(Col::PageStart), cell(Col::PageEnd)) {
            (Some(s), Some(e)) => Some(format!("{s}-{e}")),
            (s, _) => s,
        });
        if r.page.is_none() {
            r.page = cell(Col::ArticleNumber);
        } else if let Some(a) = cell(Col::ArticleNumber) {
            r.keep(FORMAT, "Art. No.", &a);
        }
        r.isbn = cell(Col::Isbn);
        r.issn = cell(Col::Issn);
        r.url = cell(Col::Url);
        r.keywords = cell(Col::Keywords)
            .map(|k| split_list(&k))
            .unwrap_or_default();
        if let Some(k) = cell(Col::IndexKeywords) {
            r.keywords.extend(split_list(&k));
        }
        r.publisher = cell(Col::Publisher);
        r.publisher_place = cell(Col::Place);
        r.language = cell(Col::Language);
        r.note = cell(Col::Note);
        r.database = cell(Col::Database);
        r.edition = cell(Col::Edition);
        for (i, v) in row.cells.iter().enumerate() {
            if known.contains(&i) || v.trim().is_empty() {
                continue;
            }
            let name = header
                .get(i)
                .map(|h| h.trim().to_string())
                .filter(|h| !h.is_empty())
                .unwrap_or_else(|| format!("column {}", i + 1));
            r.keep(FORMAT, &name, v.trim());
        }
        if let Some(eid) = id.as_deref().filter(|e| e.starts_with("2-s2.0-")) {
            r.database = r.database.take().or(Some("Scopus".into()));
            r.accession = Some(eid.to_string());
        } else if let Some(ut) = id.as_deref().filter(|u| u.starts_with("WOS:")) {
            r.accession = Some(ut.to_string());
        }
        r.id = id.unwrap_or_else(|| (index + 1).to_string());
        finish(&mut r, index, Some(row.line), diags);
        out.push(r);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc4180() {
        let mut d = Vec::new();
        let r = rows(
            "a,\"b,c\",\"d \"\"e\"\"\"\n\"multi\nline\",x\n\n",
            ',',
            true,
            &mut d,
        );
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].cells, vec!["a", "b,c", "d \"e\""]);
        assert_eq!(r[1].cells, vec!["multi\nline", "x"]);
        assert_eq!(r[1].line, 2);
        assert!(d.is_empty());
        let r = rows("\"never closed,\nmore", ',', true, &mut d);
        assert_eq!(r.len(), 1);
        assert_eq!(d[0].code, "csv.unclosed-quote");
    }

    #[test]
    fn scopus_style() {
        let src = "\u{feff}\"Authors\",\"Author full names\",\"Title\",\"Year\",\"Source title\",\"DOI\",\"Abstract\",\"PubMed ID\",\"Document Type\",\"EID\",\"Funding Details\"\n\"Okafor A.N.; Lind M.\",\"Okafor, Ada N. (1); Lind, Maja (2)\",\"Moths\",\"2019\",\"J Ecol\",\"10.5555/x\",\"[No abstract available]\",\"31000001\",\"Article\",\"2-s2.0-1\",\"none\"\n";
        let mut d = Vec::new();
        let refs = read(src.trim_start_matches('\u{feff}'), &mut d);
        assert_eq!(refs.len(), 1, "{d:?}");
        let r = &refs[0];
        assert_eq!(r.author[0].given.as_deref(), Some("Ada N."));
        assert_eq!(r.abstract_text, None);
        assert_eq!(r.accession.as_deref(), Some("2-s2.0-1"));
        assert!(r.source_fields.iter().any(|f| f.name == "Funding Details"));
    }

    #[test]
    fn semicolons_and_vancouver_authors() {
        let src = "Nr;Title;Author(s);Journal;Year;DOI\n1;Tide pools;Lind M, Okafor AN;Mar Ecol;2020;https://doi.org/10.5555/y\n";
        let mut d = Vec::new();
        let refs = read(src, &mut d);
        assert_eq!(refs[0].author.len(), 2);
        assert_eq!(refs[0].doi.as_deref(), Some("10.5555/y"));
        assert!(refs[0]
            .source_fields
            .iter()
            .any(|f| f.name == "Nr" && f.value == "1"));
    }
}
