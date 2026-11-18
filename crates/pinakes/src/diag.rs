//! Diagnostics: what a reader noticed and what it did about it.
//!
//! Readers never stop at the first problem and never drop input without a diagnostic. An
//! `Error` means part of the input could not be read as the format says, and the message says
//! what Pinakes kept instead. A `Warning` means the input was read but something in it is
//! doubtful (a DOI that does not look like one, a record with no title). `Info` notes a repair
//! that changes nothing a reader would care about (a byte-order mark, Windows-1252 text).

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Stable machine-readable code, such as `ris.missing-er` or `id.invalid-doi`.
    pub code: &'static str,
    pub message: String,
    /// 1-based line in the decoded input, when the problem has one.
    pub line: Option<usize>,
    /// 0-based index of the record in the result, when the problem belongs to one.
    pub record: Option<usize>,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

impl Diagnostic {
    pub fn new(severity: Severity, code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic { severity, code, message: message.into(), line: None, record: None }
    }

    pub fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(Severity::Error, code, message)
    }

    pub fn warning(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(Severity::Warning, code, message)
    }

    pub fn info(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(Severity::Info, code, message)
    }

    pub fn at(mut self, line: usize) -> Diagnostic {
        self.line = Some(line);
        self
    }

    pub fn record(mut self, record: usize) -> Diagnostic {
        self.record = Some(record);
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(line) = self.line {
            write!(f, "line {line}: ")?;
        }
        write!(f, "{}: {} [{}]", self.severity.as_str(), self.message, self.code)
    }
}
