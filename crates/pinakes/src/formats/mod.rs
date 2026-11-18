//! The formats Pinakes reads and writes, and how it tells them apart.

pub mod bibtex;
pub mod csl;
pub mod csv;
pub mod hayagriva;
pub mod latex;
pub mod medline;
pub mod pubmed_xml;
pub mod ris;
pub mod wos;
pub mod xml;

use crate::diag::Diagnostic;
use crate::model::Reference;
use std::fmt;
use std::str::FromStr;

/// A bibliographic file format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    /// RIS, in every dialect the reader knows.
    Ris,
    /// BibTeX. The reader also takes BibLaTeX; the writer uses BibTeX's fields and ASCII.
    Bibtex,
    /// BibLaTeX. Same reader as BibTeX; the writer uses BibLaTeX's fields and Unicode.
    Biblatex,
    /// PubMed's MEDLINE text (`.nbib`).
    Medline,
    /// PubMed XML (EFetch `PubmedArticleSet`).
    PubmedXml,
    /// CSL-JSON.
    CslJson,
    /// Web of Science plain-text export.
    WosText,
    /// Web of Science tab-delimited export.
    WosTab,
    /// A table of records: Scopus CSV exports and hand-kept lists.
    Csv,
    /// Hayagriva YAML, for Typst. Written only.
    Hayagriva,
}

impl Format {
    pub const ALL: [Format; 10] = [
        Format::Ris,
        Format::Bibtex,
        Format::Biblatex,
        Format::Medline,
        Format::PubmedXml,
        Format::CslJson,
        Format::WosText,
        Format::WosTab,
        Format::Csv,
        Format::Hayagriva,
    ];

    /// Whether one reader serves both formats, so either name reads the other's files.
    pub(crate) fn same_family(self, other: Format) -> bool {
        use Format::*;
        self == other || matches!((self, other), (Bibtex | Biblatex, Bibtex | Biblatex))
    }

    /// The name the CLI and bindings use.
    pub fn name(self) -> &'static str {
        match self {
            Format::Ris => "ris",
            Format::Bibtex => "bibtex",
            Format::Biblatex => "biblatex",
            Format::Medline => "medline",
            Format::PubmedXml => "pubmed-xml",
            Format::CslJson => "csl-json",
            Format::WosText => "wos",
            Format::WosTab => "wos-tab",
            Format::Csv => "csv",
            Format::Hayagriva => "hayagriva",
        }
    }

    pub fn can_read(self) -> bool {
        self != Format::Hayagriva
    }

    pub fn can_write(self) -> bool {
        matches!(
            self,
            Format::Ris | Format::Bibtex | Format::Biblatex | Format::CslJson | Format::Hayagriva
        )
    }

    /// The usual file extension.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Ris => "ris",
            Format::Bibtex | Format::Biblatex => "bib",
            Format::Medline => "nbib",
            Format::PubmedXml => "xml",
            Format::CslJson => "json",
            Format::WosText | Format::WosTab => "txt",
            Format::Csv => "csv",
            Format::Hayagriva => "yaml",
        }
    }

    /// The format a file extension usually means, where it means only one.
    pub fn from_extension(ext: &str) -> Option<Format> {
        Some(
            match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
                "ris" => Format::Ris,
                "bib" | "bibtex" => Format::Bibtex,
                "nbib" | "medline" => Format::Medline,
                "json" | "csljson" => Format::CslJson,
                "csv" | "tsv" => Format::Csv,
                "yaml" | "yml" => Format::Hayagriva,
                _ => return None,
            },
        )
    }

    /// Work out the format of decoded text from its first lines.
    pub fn sniff(text: &str) -> Option<Format> {
        let start = text.trim_start_matches(['\u{feff}', ' ', '\t', '\n', '\r']);
        if start.starts_with('[') || start.starts_with('{') {
            return Some(Format::CslJson);
        }
        if start.starts_with('<') {
            return Some(Format::PubmedXml);
        }
        if wos::looks_like_text(start) {
            return Some(Format::WosText);
        }
        let first = start.lines().next().unwrap_or("");
        if first.starts_with("PT\t")
            && wos::is_tab_header(&first.split('\t').map(str::to_string).collect::<Vec<_>>())
        {
            return Some(Format::WosTab);
        }
        let lines: Vec<&str> = start.lines().take(200).collect();
        if lines.iter().any(|l| l.starts_with("PMID-")) {
            return Some(Format::Medline);
        }
        if lines
            .iter()
            .any(|l| ris::tag_line(l).is_some_and(|(t, _)| t.eq_ignore_ascii_case("TY")))
        {
            return Some(Format::Ris);
        }
        if lines.iter().any(|l| is_bib_entry(l.trim_start())) {
            return Some(Format::Bibtex);
        }
        if lines.iter().filter(|l| ris::tag_line(l).is_some()).count() >= 2 {
            return Some(Format::Ris);
        }
        let rows = csv::rows(
            &lines[..lines.len().min(12)].join("\n"),
            csv::sniff_delimiter(first),
            true,
            &mut Vec::new(),
        );
        if rows
            .iter()
            .take(10)
            .any(|r| csv::is_record_header(&r.cells))
        {
            return Some(Format::Csv);
        }
        None
    }
}

fn is_bib_entry(line: &str) -> bool {
    let Some(rest) = line.strip_prefix('@') else {
        return false;
    };
    let name_len = rest.bytes().take_while(|b| b.is_ascii_alphabetic()).count();
    name_len > 0 && rest[name_len..].trim_start().starts_with(['{', '('])
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A format name that Pinakes does not know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownFormat(pub String);

impl fmt::Display for UnknownFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = Format::ALL.iter().map(|f| f.name()).collect();
        write!(
            f,
            "unknown format {:?}; the formats are {}",
            self.0,
            names.join(", ")
        )
    }
}

impl std::error::Error for UnknownFormat {}

impl FromStr for Format {
    type Err = UnknownFormat;

    fn from_str(s: &str) -> Result<Format, UnknownFormat> {
        let k: String = s
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        Ok(match k.as_str() {
            "ris" => Format::Ris,
            "bibtex" | "bib" => Format::Bibtex,
            "biblatex" => Format::Biblatex,
            "medline" | "nbib" | "pubmed" => Format::Medline,
            "pubmedxml" | "xml" => Format::PubmedXml,
            "csljson" | "csl" | "json" => Format::CslJson,
            "wos" | "wostext" | "webofscience" => Format::WosText,
            "wostab" => Format::WosTab,
            "csv" | "scopus" | "table" => Format::Csv,
            "hayagriva" | "yaml" | "typst" => Format::Hayagriva,
            _ => return Err(UnknownFormat(s.to_string())),
        })
    }
}

pub(crate) fn read_text(format: Format, text: &str, diags: &mut Vec<Diagnostic>) -> Vec<Reference> {
    match format {
        Format::Ris => ris::read(text, diags),
        Format::Bibtex | Format::Biblatex => bibtex::read(text, diags),
        Format::Medline => medline::read(text, diags),
        Format::PubmedXml => pubmed_xml::read(text, diags),
        Format::CslJson => csl::read(text, diags),
        Format::WosText => wos::read_text(text, diags),
        Format::WosTab => wos::read_tab(text, diags),
        Format::Csv => csv::read(text, diags),
        Format::Hayagriva => {
            diags.push(Diagnostic::error(
                "format.write-only",
                "Pinakes writes Hayagriva YAML but does not read it",
            ));
            Vec::new()
        }
    }
}

/// Write references, or `None` for a format Pinakes only reads.
pub(crate) fn write_text(format: Format, refs: &[Reference]) -> Option<String> {
    Some(match format {
        Format::Ris => ris::write(refs),
        Format::Bibtex => bibtex::write(refs, false),
        Format::Biblatex => bibtex::write(refs, true),
        Format::CslJson => csl::write(refs),
        Format::Hayagriva => hayagriva::write(refs),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffing() {
        assert_eq!(Format::sniff("TY  - JOUR\nER  - \n"), Some(Format::Ris));
        assert_eq!(
            Format::sniff("\n\n@article{k,\n title={x}}"),
            Some(Format::Bibtex)
        );
        assert_eq!(Format::sniff("PMID- 1\nTI  - x\n"), Some(Format::Medline));
        assert_eq!(
            Format::sniff("<?xml version=\"1.0\"?><PubmedArticleSet/>"),
            Some(Format::PubmedXml)
        );
        assert_eq!(Format::sniff("[{\"id\":\"a\"}]"), Some(Format::CslJson));
        assert_eq!(
            Format::sniff("FN Clarivate Analytics Web of Science\nVR 1.0\n"),
            Some(Format::WosText)
        );
        assert_eq!(Format::sniff("PT\tAU\tTI\n"), Some(Format::WosTab));
        assert_eq!(Format::sniff("Authors,Title,Year\n"), Some(Format::Csv));
        assert_eq!(Format::sniff("hello"), None);
        assert_eq!("BibLaTeX".parse::<Format>(), Ok(Format::Biblatex));
        assert!("docx".parse::<Format>().is_err());
    }
}
