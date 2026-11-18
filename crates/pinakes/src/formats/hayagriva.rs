//! Hayagriva YAML, the bibliography format Typst reads. Pinakes writes it and does not read it.
//!
//! Each entry is keyed by its citation key. Hayagriva nests the container: an article's
//! journal is its `parent`, of type `periodical`, and carries the volume and issue. The format
//! is described at <https://github.com/typst/hayagriva/blob/main/docs/file-format.md>.

use crate::model::{Date, Name, Reference};

/// Hayagriva's entry type and its parent's, for a CSL type.
fn types(kind: &str) -> (&'static str, Option<&'static str>) {
    match kind {
        "article-journal" | "review" | "review-book" => ("article", Some("periodical")),
        "article-magazine" => ("article", Some("periodical")),
        "article-newspaper" => ("article", Some("newspaper")),
        "article" => ("article", None),
        "book" => ("book", None),
        "chapter" | "entry-encyclopedia" | "entry-dictionary" | "entry" => ("chapter", Some("book")),
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

/// A YAML scalar, quoted whenever plain style could be misread.
fn scalar(s: &str) -> String {
    let plain = !s.is_empty()
        && !s.starts_with(|c: char| c.is_whitespace() || "-?:,[]{}#&*!|>'\"%@`~".contains(c))
        && !s.ends_with(|c: char| c.is_whitespace() || c == ':')
        && !s.contains(": ")
        && !s.contains(" #")
        && !s.chars().any(|c| c.is_control())
        && !matches!(s.to_ascii_lowercase().as_str(), "true" | "false" | "yes" | "no" | "null" | "on" | "off" | "~")
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
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn name(n: &Name) -> String {
    if let Some(l) = &n.literal {
        // Hayagriva splits a plain string at its first comma; braces keep an organisation whole.
        return if l.contains(',') { format!("{{{l}}}") } else { l.clone() };
    }
    let mut family = n.display_family();
    if let Some(d) = &n.dropping_particle {
        family = format!("{d} {family}");
    }
    let mut out = family;
    if let Some(g) = &n.given {
        out.push_str(", ");
        out.push_str(g);
    }
    if let Some(s) = &n.suffix {
        out.push_str(", ");
        out.push_str(s);
    }
    out
}

fn date(d: &Date) -> Option<String> {
    let s = d.start?;
    Some(s.iso())
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

    fn list(&mut self, indent: usize, key: &str, values: &[String]) {
        if values.is_empty() {
            return;
        }
        self.buf.push_str(&" ".repeat(indent));
        self.buf.push_str(key);
        self.buf.push_str(":\n");
        for v in values {
            self.buf.push_str(&" ".repeat(indent + 2));
            self.buf.push_str("- ");
            self.buf.push_str(&scalar(v));
            self.buf.push('\n');
        }
    }
}

/// Write references as a Hayagriva YAML document.
pub fn write(refs: &[Reference]) -> String {
    let keys = crate::key::keys_for_writing(refs);
    let mut o = Out { buf: String::new() };
    for (r, key) in refs.iter().zip(&keys) {
        let (kind, parent) = types(&r.kind);
        o.buf.push_str(&scalar(key));
        o.buf.push_str(":\n");
        o.field(2, "type", Some(kind));
        o.field(2, "title", r.title.as_deref().map(crate::text::strip_markup).as_deref());
        o.list(2, "author", &r.author.iter().map(name).collect::<Vec<_>>());
        o.field(2, "date", r.issued.as_ref().and_then(date).as_deref());
        if parent.is_none() {
            o.list(2, "editor", &r.editor.iter().map(name).collect::<Vec<_>>());
            o.field(2, "publisher", r.publisher.as_deref());
            o.field(2, "location", r.publisher_place.as_deref());
            o.field(2, "volume", r.volume.as_deref());
            o.field(2, "issue", r.issue.as_deref());
        }
        o.field(2, "edition", r.edition.as_deref());
        o.field(2, "page-range", r.page.as_deref());
        o.field(2, "genre", r.genre.as_deref());
        o.field(2, "language", r.language.as_deref());
        o.field(2, "url", r.url.as_deref());
        o.field(2, "accessed", r.accessed.as_ref().and_then(date).as_deref());
        o.field(2, "abstract", r.abstract_text.as_deref().map(crate::text::strip_markup).as_deref());
        o.field(2, "note", r.note.as_deref());
        let serial: Vec<(&str, &Option<String>)> = vec![("doi", &r.doi), ("pmid", &r.pmid), ("pmcid", &r.pmcid), ("isbn", &r.isbn), ("issn", &r.issn), ("arxiv", &r.arxiv)];
        let serial: Vec<(&str, &str)> = serial.into_iter().filter_map(|(k, v)| v.as_deref().map(|v| (k, v))).collect();
        let number = r.number.as_deref().filter(|_| parent.is_none() || r.kind != "report");
        if !serial.is_empty() || number.is_some() {
            o.buf.push_str("  serial-number:\n");
            for (k, v) in serial {
                o.field(4, k, Some(v));
            }
            o.field(4, "number", number);
        }
        let container = r.container_title.as_deref().or(r.collection_title.as_deref().filter(|_| parent.is_some()));
        if let (Some(ptype), Some(ctitle)) = (parent, container) {
            o.buf.push_str("  parent:\n");
            o.field(4, "type", Some(ptype));
            o.field(4, "title", Some(&crate::text::strip_markup(ctitle)));
            o.list(4, "editor", &r.editor.iter().map(name).collect::<Vec<_>>());
            o.field(4, "volume", r.volume.as_deref());
            o.field(4, "issue", r.issue.as_deref());
            o.field(4, "publisher", r.publisher.as_deref());
            o.field(4, "location", r.publisher_place.as_deref());
        } else if let Some(ctitle) = container {
            o.buf.push_str("  parent:\n");
            o.field(4, "title", Some(&crate::text::strip_markup(ctitle)));
        }
    }
    o.buf
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
        assert_eq!(scalar("a \"q\""), "a \"q\"");
    }
}
