//! Hayagriva YAML, the bibliography format Typst reads. Pinakes writes it and does not read it.
//!
//! Each entry is keyed by its citation key. Hayagriva nests the container: an article's
//! journal is its `parent`, of type `periodical`, and carries the volume and issue. The format
//! is described at <https://github.com/typst/hayagriva/blob/main/docs/file-format.md>.

use crate::check::{label, members, show};
use crate::diag::Diagnostic;
use crate::model::{Date, Name, Reference};

/// Hayagriva's entry type and its parent's, for a CSL type.
fn types(kind: &str) -> (&'static str, Option<&'static str>) {
    match kind {
        "article-journal" | "review" | "review-book" => ("article", Some("periodical")),
        "article-magazine" => ("article", Some("periodical")),
        "article-newspaper" => ("article", Some("newspaper")),
        "article" => ("article", None),
        "book" => ("book", None),
        "chapter" | "entry-encyclopedia" | "entry-dictionary" | "entry" => {
            ("chapter", Some("book"))
        }
        "paper-conference" => ("article", Some("proceedings")),
        "report" => ("report", None),
        "thesis" => ("thesis", None),
        "webpage" | "post-weblog" => ("web", None),
        "post" => ("post", None),
        "patent" => ("patent", None),
        "legal_case" => ("case", None),
        "legislation" | "bill" | "regulation" => ("legislation", None),
        "manuscript" => ("manuscript", None),
        "dataset" | "software" => ("repository", None),
        "motion_picture" | "broadcast" => ("video", None),
        "song" => ("audio", None),
        "graphic" => ("artwork", None),
        "periodical" => ("periodical", None),
        "standard" => ("report", None),
        _ => ("misc", None),
    }
}

/// The CSL types Hayagriva's types stand for: a type not listed here shares its Hayagriva
/// type with one that is, and so cannot be told from it.
const OWN_TYPES: &[&str] = &[
    "article-journal",
    "article-newspaper",
    "article",
    "book",
    "chapter",
    "paper-conference",
    "report",
    "thesis",
    "webpage",
    "post",
    "patent",
    "legal_case",
    "legislation",
    "manuscript",
    "dataset",
    "motion_picture",
    "song",
    "graphic",
    "periodical",
    "document",
];

/// What Hayagriva's person cannot hold of a name, if anything.
fn name_loss(n: &Name) -> Option<&'static str> {
    if n.is_others() {
        Some("Hayagriva has no marker for names left out; wrote it as a person named \"others\"")
    } else if !n.extra.is_empty() {
        Some("Hayagriva has no place for its other members; left them out")
    } else if n.dropping_particle.is_some() {
        Some("Hayagriva has one prefix; wrote the dropping particle as it")
    } else if n.literal.is_some()
        && (n.family.is_some() || n.given.is_some() || n.particle.is_some() || n.suffix.is_some())
    {
        Some("Hayagriva holds a literal name or its parts, not both; wrote the literal")
    } else if n.literal.is_none() && n.family.is_none() {
        Some("Hayagriva needs a family name; wrote the name with an empty one")
    } else {
        None
    }
}

/// What Hayagriva's date cannot hold of a date, if anything.
fn date_loss(d: &Date) -> Option<&'static str> {
    if date(d).is_none() {
        Some("Hayagriva holds dates with a year from 0 to 9999; left it out")
    } else if d.end.is_some() {
        Some("Hayagriva has no date ranges; wrote the start")
    } else if d.literal.is_some() || d.raw.is_some() || !d.extra.is_empty() {
        Some("Hayagriva holds only the year, month, day and circa; wrote those")
    } else {
        None
    }
}

/// A YAML scalar. Plain style only for text that can be nothing but a string (it starts
/// with a letter and holds no YAML punctuation); anything else is double-quoted with every
/// character YAML could fold or refuse escaped, so no value reads as a number, a boolean, a
/// document marker or a second key.
fn scalar(s: &str) -> String {
    let plain = s.starts_with(|c: char| c.is_alphabetic())
        && s.chars()
            .all(|c| c.is_alphanumeric() || " .,;()/_+-'".contains(c))
        && !s.ends_with([' ', '.'])
        && !s.contains("  ")
        && !matches!(
            s.to_ascii_lowercase().as_str(),
            "true" | "false" | "yes" | "no" | "y" | "n" | "null" | "on" | "off"
        )
        && s.parse::<f64>().is_err();
    if plain {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control()
                || matches!(
                    c,
                    '\u{2028}' | '\u{2029}' | '\u{feff}' | '\u{fffe}' | '\u{ffff}'
                ) =>
            {
                out.push_str(&format!("\\u{:04x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Text for a Hayagriva formatted string, where `{`, `}`, `$` and `\` are markup.
fn formatted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '{' | '}' | '$') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A name Hayagriva's short form "Family, Given, Suffix" reads back unchanged: no comma, no
/// lower-case word it would take for a prefix.
fn short_form(n: &Name) -> Option<String> {
    let family = n.family.as_deref()?;
    let simple = |s: &str| {
        !s.is_empty()
            && !s.contains(',')
            && s.split_whitespace()
                .all(|w| w.starts_with(|c: char| c.is_uppercase()))
            && s.trim() == s
    };
    if n.literal.is_some()
        || n.particle.is_some()
        || n.dropping_particle.is_some()
        || n.suffix.is_some()
        || !simple(family)
        || !n.given.as_deref().is_none_or(simple)
    {
        return None;
    }
    Some(match &n.given {
        Some(g) => format!("{family}, {g}"),
        None => family.to_string(),
    })
}

fn date(d: &Date) -> Option<String> {
    let s = d.start?;
    // Hayagriva reads years 0 to 9999; `~` marks an approximate date.
    if !(0..=9999).contains(&s.year) {
        return None;
    }
    Some(if d.circa {
        format!("~{}", s.iso())
    } else {
        s.iso()
    })
}

/// Hayagriva refuses a whole file over one URL it cannot parse; write only URLs that are
/// plainly URLs.
fn url_like(u: &str) -> bool {
    let Some((scheme, rest)) = u.split_once("://") else {
        return false;
    };
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..host_end];
    matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "ftp"
    ) && !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == ':')
        && !host.starts_with(['.', '-', ':'])
        && rest[host_end..]
            .chars()
            .all(|c| c.is_ascii_graphic() && !"<>\"{}|\\^`".contains(c))
}

/// A BCP 47 tag Hayagriva's language parser takes: a 2-3 letter language, then a region.
fn language_tag(l: &str) -> bool {
    let mut parts = l.split(['-', '_']);
    let lang = parts.next().unwrap_or_default();
    (2..=3).contains(&lang.len())
        && lang.chars().all(|c| c.is_ascii_alphabetic())
        && parts.all(|p| {
            (p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()))
                || (p.len() == 4 && p.chars().all(|c| c.is_ascii_alphabetic()))
        })
}

struct Out {
    buf: String,
}

impl Out {
    fn field(&mut self, indent: usize, key: &str, value: Option<&str>) {
        if let Some(v) = value.filter(|v| !v.trim().is_empty()) {
            self.buf.push_str(&" ".repeat(indent));
            self.buf.push_str(key);
            self.buf.push_str(": ");
            self.buf.push_str(&scalar(v));
            self.buf.push('\n');
        }
    }

    /// A formatted-string field.
    fn text(&mut self, indent: usize, key: &str, value: Option<&str>) {
        self.field(indent, key, value.map(formatted).as_deref());
    }

    /// Persons: the short string where it reads back the same, otherwise a dictionary.
    fn persons(&mut self, indent: usize, key: &str, names: &[Name]) {
        if names.is_empty() {
            return;
        }
        self.buf.push_str(&" ".repeat(indent));
        self.buf.push_str(key);
        self.buf.push_str(":\n");
        let pad = " ".repeat(indent + 2);
        for n in names {
            if let Some(short) = short_form(n) {
                self.buf.push_str(&format!("{pad}- {}\n", scalar(&short)));
                continue;
            }
            let family = n
                .literal
                .clone()
                .unwrap_or_else(|| n.family.clone().unwrap_or_default());
            self.buf
                .push_str(&format!("{pad}- name: {}\n", scalar(&family)));
            let prefix = match (&n.dropping_particle, &n.particle) {
                (Some(d), Some(p)) => Some(format!("{d} {p}")),
                (d, p) => d.clone().or_else(|| p.clone()),
            };
            for (k, v) in [
                ("given-name", n.given.clone()),
                ("prefix", prefix),
                ("suffix", n.suffix.clone()),
            ] {
                if let (Some(v), None) = (v, &n.literal) {
                    self.buf.push_str(&format!("{pad}  {k}: {}\n", scalar(&v)));
                }
            }
        }
    }
}

/// Write references as a Hayagriva YAML document.
pub fn write(refs: &[Reference], diags: &mut Vec<Diagnostic>) -> String {
    let keys = crate::key::keys_for_writing(refs);
    let mut o = Out { buf: String::new() };
    for (i, (r, key)) in refs.iter().zip(&keys).enumerate() {
        let (kind, parent) = types(&r.kind);
        report(r, i, key, diags);
        o.buf.push_str(&scalar(key));
        o.buf.push_str(":\n");
        o.field(2, "type", Some(kind));
        o.text(
            2,
            "title",
            r.title.as_deref().map(crate::text::strip_markup).as_deref(),
        );
        o.persons(2, "author", &r.author);
        o.field(2, "date", r.issued.as_ref().and_then(date).as_deref());
        if parent.is_none() {
            o.persons(2, "editor", &r.editor);
            o.text(2, "publisher", r.publisher.as_deref());
            o.text(2, "location", r.publisher_place.as_deref());
            o.field(2, "volume", r.volume.as_deref());
            o.field(2, "issue", r.issue.as_deref());
        }
        o.field(2, "edition", r.edition.as_deref());
        o.field(2, "page-range", r.page.as_deref());
        o.text(2, "genre", r.genre.as_deref());
        o.field(
            2,
            "language",
            r.language.as_deref().filter(|l| language_tag(l)),
        );
        let url = r.url.as_deref().filter(|u| url_like(u));
        o.field(2, "url", url);
        if url.is_some() {
            o.field(2, "accessed", r.accessed.as_ref().and_then(date).as_deref());
        }
        o.text(
            2,
            "abstract",
            r.abstract_text
                .as_deref()
                .map(crate::text::strip_markup)
                .as_deref(),
        );
        o.text(2, "note", r.note.as_deref());
        let serial: Vec<(&str, &Option<String>)> = vec![
            ("doi", &r.doi),
            ("pmid", &r.pmid),
            ("pmcid", &r.pmcid),
            ("isbn", &r.isbn),
            ("issn", &r.issn),
            ("arxiv", &r.arxiv),
        ];
        let serial: Vec<(&str, &str)> = serial
            .into_iter()
            .filter_map(|(k, v)| v.as_deref().map(|v| (k, v)))
            .collect();
        let number = r
            .number
            .as_deref()
            .filter(|_| parent.is_none() || r.kind != "report");
        if !serial.is_empty() || number.is_some() {
            o.buf.push_str("  serial-number:\n");
            for (k, v) in serial {
                o.field(4, k, Some(v));
            }
            o.field(4, "number", number);
        }
        let container = r
            .container_title
            .as_deref()
            .or(r.collection_title.as_deref().filter(|_| parent.is_some()))
            .map(crate::text::strip_markup)
            .filter(|c| !c.trim().is_empty());
        if let (Some(ptype), Some(ctitle)) = (parent, &container) {
            o.buf.push_str("  parent:\n");
            o.field(4, "type", Some(ptype));
            o.text(4, "title", Some(ctitle));
            o.persons(4, "editor", &r.editor);
            o.field(4, "volume", r.volume.as_deref());
            o.field(4, "issue", r.issue.as_deref());
            o.text(4, "publisher", r.publisher.as_deref());
            o.text(4, "location", r.publisher_place.as_deref());
        } else if let Some(ctitle) = container {
            o.buf.push_str("  parent:\n");
            o.text(4, "title", Some(&ctitle));
        }
    }
    o.buf
}

/// A diagnostic for each member of `r` that [`write`] does not write as it is.
fn report(r: &Reference, i: usize, key: &str, diags: &mut Vec<Diagnostic>) {
    let (kind, parent) = types(&r.kind);
    let container = r
        .container_title
        .as_deref()
        .or(r.collection_title.as_deref().filter(|_| parent.is_some()))
        .map(crate::text::strip_markup)
        .filter(|c| !c.trim().is_empty());
    // With a parent type but no container title there is no parent to hold these.
    let orphaned = parent.is_some() && container.is_none();
    let label = label(r, i);
    let left_out = "Hayagriva has no place for it; left it out";
    for (k, v) in members(r) {
        let text = v.as_str().unwrap_or_default();
        let (code, what): (&'static str, String) = match k.as_str() {
            "type" if OWN_TYPES.contains(&text) => continue,
            "type" => (
                "hayagriva.type",
                format!("Hayagriva has no type of its own for it; wrote {kind}"),
            ),
            "citation-key" if text == key => continue,
            "citation-key" => (
                "hayagriva.changed",
                format!("keys must be unique and plain; wrote {key:?}"),
            ),
            "title" | "abstract" | "container-title" if crate::text::strip_markup(text) != text => {
                (
                    "hayagriva.markup",
                    "Hayagriva has no markup; wrote the text without it".to_string(),
                )
            }
            "collection-title" if !(parent.is_some() && r.container_title.is_none()) => {
                ("hayagriva.left-out", left_out.to_string())
            }
            "editor" | "publisher" | "publisher-place" | "volume" | "issue" if orphaned => (
                "hayagriva.left-out",
                "it belongs to the container, and the record names none; left it out".to_string(),
            ),
            "author" | "editor" => {
                let names = if k == "author" { &r.author } else { &r.editor };
                match names.iter().find_map(|n| name_loss(n).map(|w| (n, w))) {
                    Some((n, w)) => ("hayagriva.name", format!("{}: {w}", n.inverted())),
                    None => continue,
                }
            }
            "issued" | "accessed" => {
                let d = if k == "issued" {
                    &r.issued
                } else {
                    &r.accessed
                };
                let url = r.url.as_deref().is_some_and(url_like);
                match d.as_ref().and_then(date_loss) {
                    _ if k == "accessed" && !url => (
                        "hayagriva.left-out",
                        "Hayagriva holds an access date only with a URL; left it out".to_string(),
                    ),
                    Some(w) => ("hayagriva.changed", w.to_string()),
                    None => continue,
                }
            }
            "language" if !language_tag(text) => (
                "hayagriva.left-out",
                "Hayagriva takes only a language tag such as \"en\" here; left it out".to_string(),
            ),
            "URL" if !url_like(text) => (
                "hayagriva.left-out",
                "Hayagriva refuses a file over a URL it cannot parse; left it out".to_string(),
            ),
            "title" | "abstract" | "container-title" | "collection-title" | "publisher"
            | "publisher-place" | "volume" | "issue" | "edition" | "page" | "genre" | "note"
            | "language" | "URL" | "DOI" | "PMID" | "PMCID" | "ISBN" | "ISSN" | "custom.arxiv"
            | "number" => {
                if text.trim().is_empty() {
                    ("hayagriva.left-out", "it is blank; left it out".to_string())
                } else {
                    continue;
                }
            }
            _ => ("hayagriva.left-out", left_out.to_string()),
        };
        diags.push(
            Diagnostic::warning(code, format!("{label}: {k} {}: {what}", show(&v)))
                .record(i)
                .field(k),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(scalar("Moths"), "Moths");
        assert_eq!(scalar("Title: subtitle"), "\"Title: subtitle\"");
        assert_eq!(scalar("2019"), "\"2019\"");
        assert_eq!(scalar("yes"), "\"yes\"");
        assert_eq!(scalar("- dash"), "\"- dash\"");
        assert_eq!(scalar("a \"q\""), "\"a \\\"q\\\"\"");
        assert_eq!(scalar("---"), "\"---\"");
        assert_eq!(scalar("..."), "\"...\"");
        assert_eq!(scalar("NaN"), "\"NaN\"");
        assert_eq!(scalar("a\u{2028}b"), "\"a\\u2028b\"");
        assert_eq!(formatted("{x} $y$ \\"), "\\{x\\} \\$y\\$ \\\\");
    }
}
