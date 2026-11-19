//! The JSON form of what Pinakes returns besides references, which travel as CSL-JSON. The CLI
//! and both bindings print exactly this, so a diagnostic or a duplicate looks the same in every
//! language.
//!
//! ```json
//! {"severity": "warning", "code": "id.invalid-doi", "message": "…", "line": 12, "record": 3}
//! {"a": 0, "b": 4, "relation": "duplicate", "score": 100.0, "exact": true,
//!  "reasons": [{"kind": "identifier", "identifier": "doi", "value": "10.5555/x",
//!               "text": "Same DOI 10.5555/x"}]}
//! ```

use crate::dedupe::{Candidate, Duplicates, Group, Reason, Relation};
use crate::diag::{Diagnostic, Severity};
use crate::formats::Format;
use crate::id::IdKind;
use crate::model::Reference;
use crate::{Library, Written};
use serde::ser::{Serialize, SerializeMap, Serializer};

macro_rules! by_name {
    ($($t:ty => $f:expr),* $(,)?) => {$(
        impl Serialize for $t {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str($f(*self))
            }
        }
    )*};
}

by_name! {
    Severity => Severity::as_str,
    Relation => Relation::name,
    IdKind => IdKind::name,
    Format => Format::name,
}

impl Serialize for Diagnostic {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(6))?;
        m.serialize_entry("severity", &self.severity)?;
        m.serialize_entry("code", self.code)?;
        m.serialize_entry("message", &self.message)?;
        m.serialize_entry("line", &self.line)?;
        m.serialize_entry("record", &self.record)?;
        m.serialize_entry("field", &self.field)?;
        m.end()
    }
}

impl Serialize for Reason {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(None)?;
        match self {
            Reason::Identifier { kind, value } => {
                m.serialize_entry("kind", "identifier")?;
                m.serialize_entry("identifier", kind)?;
                m.serialize_entry("value", value)?;
            }
            Reason::Title { score } => {
                m.serialize_entry("kind", "title")?;
                m.serialize_entry("score", score)?;
            }
            Reason::Year { year } => {
                m.serialize_entry("kind", "year")?;
                m.serialize_entry("year", year)?;
            }
            Reason::FirstAuthor { family } => {
                m.serialize_entry("kind", "first-author")?;
                m.serialize_entry("family", family)?;
            }
        }
        m.serialize_entry("text", &self.describe())?;
        m.end()
    }
}

impl Serialize for Candidate {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(6))?;
        m.serialize_entry("a", &self.a)?;
        m.serialize_entry("b", &self.b)?;
        m.serialize_entry("relation", &self.relation)?;
        m.serialize_entry("score", &((self.score * 100.0).round() / 100.0))?;
        m.serialize_entry("exact", &self.exact())?;
        m.serialize_entry("reasons", &self.reasons)?;
        m.end()
    }
}

impl Serialize for Group {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(2))?;
        m.serialize_entry("members", &self.members)?;
        m.serialize_entry("exact", &self.exact)?;
        m.end()
    }
}

impl Serialize for Duplicates {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(2))?;
        m.serialize_entry("pairs", &self.pairs)?;
        m.serialize_entry("groups", &self.groups)?;
        m.end()
    }
}

/// A reference serialises as a CSL-JSON item, unknown fields included.
impl Serialize for Reference {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        crate::formats::csl::to_value(self).serialize(s)
    }
}

impl Serialize for Library {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(3))?;
        m.serialize_entry("format", &self.format)?;
        m.serialize_entry("references", &self.references)?;
        m.serialize_entry("diagnostics", &self.diagnostics)?;
        m.end()
    }
}

impl Serialize for Written {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(2))?;
        m.serialize_entry("text", &self.text)?;
        m.serialize_entry("diagnostics", &self.diagnostics)?;
        m.end()
    }
}

#[cfg(test)]
mod tests {
    use crate::{find_duplicates, read};

    #[test]
    fn shapes() {
        let ris = b"TY  - JOUR\nTI  - Moths\nDO  - 10.5555/X\nER  - \nTY  - JOUR\nTI  - Moths again\nDO  - 10.5555/x\nER  - \n";
        let lib = read(ris, None);
        let v = serde_json::to_value(&lib).unwrap();
        assert_eq!(v["format"], "ris");
        assert_eq!(v["references"][0]["DOI"], "10.5555/x");
        let d = serde_json::to_value(find_duplicates(&lib.references)).unwrap();
        assert_eq!(d["pairs"][0]["relation"], "duplicate");
        assert_eq!(d["pairs"][0]["reasons"][0]["identifier"], "doi");
        assert_eq!(d["pairs"][0]["reasons"][0]["text"], "Same DOI 10.5555/x");
        assert_eq!(d["groups"][0]["members"], serde_json::json!([0, 1]));
    }
}
