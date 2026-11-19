//! The reference model.
//!
//! A [`Reference`] holds the fields that CSL-JSON names, with typed slots for the ones Pinakes
//! reads, writes or compares. Everything else survives in two places: [`Reference::extra`] keeps
//! CSL-JSON fields Pinakes does not model, and [`Reference::source_fields`] keeps fields from the
//! source format that have no CSL home (an RIS `QZ` tag, a BibTeX `owner` field). Writers put
//! both back out where the target format can hold them.

use serde_json::{Map, Value};

/// One bibliographic record.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reference {
    /// The record's identifier in its source (RIS `ID`, BibTeX key, CSL `id`), or its ordinal.
    pub id: String,
    /// CSL item type, such as `article-journal`, `book` or `chapter`.
    pub kind: String,
    /// CSL `citation-key`: the key a writer uses when the target format needs one.
    pub citation_key: Option<String>,
    pub title: Option<String>,
    pub title_short: Option<String>,
    pub author: Vec<Name>,
    pub editor: Vec<Name>,
    pub issued: Option<Date>,
    pub accessed: Option<Date>,
    pub container_title: Option<String>,
    pub container_title_short: Option<String>,
    pub collection_title: Option<String>,
    pub publisher: Option<String>,
    pub publisher_place: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub number: Option<String>,
    pub page: Option<String>,
    pub edition: Option<String>,
    pub genre: Option<String>,
    pub abstract_text: Option<String>,
    pub language: Option<String>,
    pub note: Option<String>,
    pub keywords: Vec<String>,
    pub url: Option<String>,
    pub doi: Option<String>,
    pub pmid: Option<String>,
    pub pmcid: Option<String>,
    pub isbn: Option<String>,
    pub issn: Option<String>,
    pub arxiv: Option<String>,
    /// Database accession number (RIS `AN`, Web of Science `UT`, Scopus `EID`).
    pub accession: Option<String>,
    /// The database or catalogue the record came from, as the export names it.
    pub database: Option<String>,
    /// Publication types as the source lists them (PubMed `PT`, Web of Science `DT`).
    pub publication_types: Vec<String>,
    /// CSL-JSON fields that have no typed slot, kept as they were read.
    pub extra: Map<String, Value>,
    /// Source fields with no CSL home, in source order, repeats kept.
    pub source_fields: Vec<SourceField>,
}

/// A field from the source format that Pinakes keeps but does not interpret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceField {
    /// Format the field came from: `ris`, `bibtex`, `medline`, `wos`, `csv`, `pubmed-xml`.
    pub format: String,
    pub name: String,
    pub value: String,
}

/// A person or organisation, in CSL's parts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Name {
    pub family: Option<String>,
    pub given: Option<String>,
    /// CSL `non-dropping-particle`: "van der" in "van der Berg, Jan".
    pub particle: Option<String>,
    /// CSL `dropping-particle`.
    pub dropping_particle: Option<String>,
    pub suffix: Option<String>,
    /// An organisation, or a name that should not be split.
    pub literal: Option<String>,
    /// CSL-JSON name members Pinakes does not model (`static-ordering`, `comma-suffix`, ...),
    /// kept as they were read.
    pub extra: Map<String, Value>,
}

/// A date or date range. `raw` keeps the source text when parsing it lost something.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Date {
    pub start: Option<Ymd>,
    pub end: Option<Ymd>,
    pub circa: bool,
    /// A date with no year to read ("in press", "forthcoming").
    pub literal: Option<String>,
    /// The source text, when `start` and `end` do not say all of it ("Summer 2020").
    pub raw: Option<String>,
    /// CSL-JSON date members Pinakes does not model (`season`, ...), kept as they were read.
    pub extra: Map<String, Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ymd {
    pub year: i32,
    pub month: Option<u8>,
    pub day: Option<u8>,
}

impl Reference {
    /// The year the record was issued, if any.
    pub fn year(&self) -> Option<i32> {
        self.issued.as_ref().and_then(|d| d.start).map(|d| d.year)
    }

    /// Add a source field.
    pub fn keep(&mut self, format: &str, name: &str, value: &str) {
        self.source_fields.push(SourceField {
            format: format.to_string(),
            name: name.to_string(),
            value: value.to_string(),
        });
    }

    /// Source fields from one format.
    pub fn source_fields_of<'a>(
        &'a self,
        format: &'a str,
    ) -> impl Iterator<Item = &'a SourceField> {
        self.source_fields
            .iter()
            .filter(move |f| f.format == format)
    }
}

impl Name {
    /// The literal that stands for names left out ("and others", "et al."). BibTeX writes it
    /// back as `and others`, RIS as `et al.`; CSL-JSON holds it as `{"literal": "others"}`.
    pub const OTHERS: &'static str = "others";

    /// The marker for names left out.
    pub fn others() -> Name {
        Name::literal(Name::OTHERS)
    }

    /// Whether this is the marker for names left out.
    pub fn is_others(&self) -> bool {
        self.literal.as_deref() == Some(Name::OTHERS)
            && self.family.is_none()
            && self.given.is_none()
    }

    pub fn literal(text: &str) -> Name {
        Name {
            literal: Some(text.trim().to_string()),
            ..Name::default()
        }
    }

    pub fn person(family: &str, given: Option<&str>) -> Name {
        Name {
            family: Some(family.trim().to_string()),
            given: given
                .map(|g| g.trim().to_string())
                .filter(|g| !g.is_empty()),
            ..Name::default()
        }
    }

    /// The family name with its particle, or the literal: what a reader would sort under.
    pub fn display_family(&self) -> String {
        if let Some(lit) = &self.literal {
            return lit.clone();
        }
        match (&self.particle, &self.family) {
            (Some(p), Some(f)) => format!("{p} {f}"),
            (None, Some(f)) => f.clone(),
            (Some(p), None) => p.clone(),
            (None, None) => String::new(),
        }
    }

    /// "Family, Given, Suffix", the form RIS and BibTeX both take.
    pub fn inverted(&self) -> String {
        if let Some(lit) = &self.literal {
            return lit.clone();
        }
        let family = self.display_family();
        let mut out = match &self.dropping_particle {
            Some(d) if !family.is_empty() => format!("{d} {family}"),
            Some(d) => d.clone(),
            None => family,
        };
        if let Some(g) = &self.given {
            out.push_str(", ");
            out.push_str(g);
        }
        if let Some(s) = &self.suffix {
            // With no given name the suffix keeps its place: "Family, , Suffix".
            out.push_str(if self.given.is_some() { ", " } else { ", , " });
            out.push_str(s);
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.family.is_none() && self.given.is_none() && self.literal.is_none()
    }
}

impl Date {
    pub fn year(year: i32) -> Date {
        Date {
            start: Some(Ymd {
                year,
                month: None,
                day: None,
            }),
            ..Date::default()
        }
    }

    pub fn ymd(year: i32, month: Option<u8>, day: Option<u8>) -> Date {
        Date {
            start: Some(Ymd { year, month, day }),
            ..Date::default()
        }
    }

    /// ISO 8601 form of the start (and end): `2020`, `2020-05`, `2020-05-01/2020-06`.
    pub fn iso(&self) -> Option<String> {
        let start = self.start?;
        let mut out = start.iso();
        if let Some(end) = self.end {
            out.push('/');
            out.push_str(&end.iso());
        }
        Some(out)
    }
}

impl Ymd {
    pub fn iso(&self) -> String {
        let mut out = if self.year < 0 {
            format!("-{:04}", -self.year)
        } else {
            format!("{:04}", self.year)
        };
        if let Some(m) = self.month {
            out.push_str(&format!("-{m:02}"));
            if let Some(d) = self.day {
                out.push_str(&format!("-{d:02}"));
            }
        }
        out
    }
}
