//! Pinakes reads, writes, normalises, keys and de-duplicates bibliographic references.
//!
//! Five calls cover it:
//!
//! - [`read`] turns the bytes of a file in any [`Format`] it reads into [`Reference`]s and
//!   [`Diagnostic`]s. It never panics and never drops input without a diagnostic.
//! - [`write`] turns references into text in a format it writes.
//! - [`normalise`] puts a reference's identifiers and page ranges in normal form. [`read`]
//!   has already done this for what it returns.
//! - [`assign_keys`] gives references citation keys.
//! - [`find_duplicates`] proposes records that are the same publication.
//!
//! ```
//! let lib = pinakes::read(b"TY  - JOUR\nTI  - Moths\nAU  - Okafor, Ada\nPY  - 2019\nDO  - https://doi.org/10.5555/X\nER  - \n", None);
//! assert_eq!(lib.references[0].doi.as_deref(), Some("10.5555/x"));
//! let bib = pinakes::write(&lib.references, pinakes::Format::Biblatex).unwrap();
//! assert!(bib.starts_with("@article{okafor2019moths,"));
//! ```

pub mod dates;
pub mod dedupe;
pub mod diag;
pub mod formats;
pub mod id;
pub mod key;
pub mod model;
pub mod names;
pub mod normalise;
pub mod text;

pub use dedupe::{find_duplicates, Candidate, Duplicates, Group, Reason, Relation};
pub use diag::{Diagnostic, Severity};
pub use formats::{Format, UnknownFormat};
pub use key::{assign_keys, key_for};
pub use model::{Date, Name, Reference, SourceField, Ymd};
pub use normalise::normalise;

/// What [`read`] returns.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Library {
    pub references: Vec<Reference>,
    /// Problems found while reading, in the order found. Each has a line number where the
    /// format has lines, and the index of its record where it belongs to one.
    pub diagnostics: Vec<Diagnostic>,
    /// The format read: the one asked for, or the one recognised.
    pub format: Option<Format>,
}

impl Library {
    /// Whether any diagnostic is an error: input that was skipped or could not be read as meant.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

/// Read references from the bytes of a file. With `format` unset, the format is recognised
/// from the content. Bytes are decoded as UTF-8, UTF-16 with or without a byte-order mark, or
/// Windows-1252 (Latin-1), and line endings are normalised.
pub fn read(bytes: &[u8], format: Option<Format>) -> Library {
    let mut diagnostics = Vec::new();
    let text = text::decode(bytes, &mut diagnostics);
    read_decoded(&text, format, diagnostics)
}

/// [`read`] for text that is already decoded.
pub fn read_str(text: &str, format: Option<Format>) -> Library {
    let mut diagnostics = Vec::new();
    let text = text::clean_text(text.to_string(), &mut diagnostics);
    read_decoded(&text, format, diagnostics)
}

fn read_decoded(text: &str, format: Option<Format>, mut diagnostics: Vec<Diagnostic>) -> Library {
    let format = match format.or_else(|| Format::sniff(text)) {
        Some(f) => f,
        None => {
            if !text.trim().is_empty() {
                diagnostics.push(Diagnostic::error(
                    "format.unknown",
                    "Could not tell the format of the input; name it explicitly",
                ));
            }
            return Library {
                references: Vec::new(),
                diagnostics,
                format: None,
            };
        }
    };
    let references = formats::read_text(format, text, &mut diagnostics);
    Library {
        references,
        diagnostics,
        format: Some(format),
    }
}

/// A format that Pinakes reads but does not write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadOnlyFormat(pub Format);

impl std::fmt::Display for ReadOnlyFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pinakes reads {} but does not write it; it writes ris, bibtex, biblatex, csl-json and hayagriva", self.0)
    }
}

impl std::error::Error for ReadOnlyFormat {}

/// Write references in a format. Fields the format has no place for are dropped, except source
/// fields that came from the same format, which are written back as they were read. Records
/// without a citation key get one from [`key_for`] in formats that need keys.
pub fn write(refs: &[Reference], format: Format) -> Result<String, ReadOnlyFormat> {
    formats::write_text(format, refs).ok_or(ReadOnlyFormat(format))
}
