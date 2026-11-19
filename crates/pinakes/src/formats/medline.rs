//! PubMed's MEDLINE format (`.nbib`, EFetch `rettype=medline`).
//!
//! A tag of up to four characters padded to four, `- `, then the value. A value continues on
//! lines indented by six spaces. Each record starts with `PMID-`, and blank lines separate
//! records. The element list is at <https://www.nlm.nih.gov/bsd/mms/medlineelements.html>.

use crate::dates::parse_date;
use crate::diag::Diagnostic;
use crate::model::{Name, Reference};
use crate::names::{expand_initials, parse_name};
use crate::normalise::finish;

pub const FORMAT: &str = "medline";

/// `TAG - value` with the tag padded to four characters. Returns the tag and the value.
pub(crate) fn tag_line(line: &str) -> Option<(&str, &str)> {
    let b = line.as_bytes();
    if b.len() < 5 || b[4] != b'-' || !b[0].is_ascii_uppercase() {
        return None;
    }
    let tag = line[..4].trim_end();
    if tag.is_empty()
        || !tag
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        || line[..4][tag.len()..].bytes().any(|c| c != b' ')
    {
        return None;
    }
    match b.get(5) {
        None => Some((tag, "")),
        Some(b' ') => Some((tag, line[6..].trim_end())),
        _ => None,
    }
}

struct Raw {
    line: usize,
    fields: Vec<(String, String, usize)>,
}

fn records(text: &str, diags: &mut Vec<Diagnostic>) -> Vec<Raw> {
    let mut out: Vec<Raw> = Vec::new();
    let mut open = false;
    for (n, line) in text.split('\n').enumerate() {
        let n = n + 1;
        if line.trim().is_empty() {
            open = false;
            continue;
        }
        if let Some((tag, value)) = tag_line(line) {
            if tag == "PMID" || !open {
                if tag != "PMID" {
                    diags.push(
                        Diagnostic::warning("medline.no-pmid", "A record does not start with PMID")
                            .at(n),
                    );
                }
                out.push(Raw {
                    line: n,
                    fields: Vec::new(),
                });
                open = true;
            }
            out.last_mut()
                .unwrap()
                .fields
                .push((tag.to_string(), value.to_string(), n));
            continue;
        }
        let continuation = line.starts_with("      ") || line.starts_with('\t');
        match out
            .last_mut()
            .and_then(|r| r.fields.last_mut())
            .filter(|_| open)
        {
            Some(last) => {
                if !continuation {
                    diags.push(Diagnostic::warning("medline.bad-line", "A line is neither a field nor an indented continuation; read it as a continuation").at(n));
                }
                if !last.1.is_empty() {
                    last.1.push(' ');
                }
                last.1.push_str(line.trim());
            }
            None => diags.push(
                Diagnostic::error("medline.stray-text", "Text outside any record; skipped it")
                    .at(n),
            ),
        }
    }
    out
}

fn to_reference(raw: Raw, index: usize, diags: &mut Vec<Diagnostic>) -> Reference {
    let mut r = Reference {
        kind: "article-journal".into(),
        ..Reference::default()
    };
    let mut pending_full: Option<Name> = None;
    let mut fau_seen = false;
    let mut editors_full: Option<Name> = None;
    let mut book_title = None;
    let mut title = None;
    let mut dates = (None, None);
    let mut bare_ed = false;
    for (tag, value, line) in raw.fields {
        let v = value.trim();
        match tag.as_str() {
            "PMID" => r.pmid = Some(v.to_string()),
            "TI" => title = Some(v.to_string()),
            "BTI" => book_title = Some(v.to_string()),
            "CTI" => r.collection_title = Some(v.to_string()),
            "AB" => r.abstract_text = Some(v.to_string()),
            "FAU" => {
                if let Some(n) = pending_full.take() {
                    r.author.push(n);
                }
                pending_full = Some(parse_name(v));
                fau_seen = true;
            }
            "AU" => match pending_full.take() {
                Some(n) => r.author.push(n),
                None => {
                    if fau_seen {
                        diags.push(
                            Diagnostic::info(
                                "medline.au-without-fau",
                                format!("Author {v:?} has no full name (FAU)"),
                            )
                            .at(line)
                            .record(index),
                        );
                    }
                    r.author.push(parse_name(v));
                }
            },
            "CN" => {
                if let Some(n) = pending_full.take() {
                    r.author.push(n);
                }
                r.author.push(Name::literal(v));
            }
            "FED" => {
                let full = parse_name(v);
                if let Some(n) = editors_full.take() {
                    r.editor.push(n);
                }
                // Some exports put ED before its FED; the full name replaces the short one.
                if bare_ed && r.editor.last().is_some_and(|e| e.family == full.family) {
                    r.editor.pop();
                    r.editor.push(full);
                } else {
                    editors_full = Some(full);
                }
                bare_ed = false;
            }
            "ED" => {
                bare_ed = editors_full.is_none();
                match editors_full.take() {
                    Some(n) => r.editor.push(n),
                    None => r.editor.push(parse_name(v)),
                }
            }
            "DP" => dates.0 = parse_date(v),
            "DEP" => {
                dates.1 = parse_date(v);
                r.keep(FORMAT, &tag, v);
            }
            "TA" => r.container_title_short = Some(v.to_string()),
            "JT" => r.container_title = Some(v.to_string()),
            "VI" => r.volume = Some(v.to_string()),
            "IP" => r.issue = Some(v.to_string()),
            "PG" => r.page = Some(v.to_string()),
            "LA" if r.language.is_none() => r.language = Some(v.to_string()),
            "PT" => r.publication_types.push(v.to_string()),
            "OT" => r.keywords.push(v.to_string()),
            "PMC" => r.pmcid = Some(v.to_string()),
            "IS" => {
                let (num, _) = v.split_once(' ').unwrap_or((v, ""));
                if r.issn.is_none() && crate::id::issn(num).is_ok() {
                    r.issn = Some(num.to_string());
                } else {
                    r.keep(FORMAT, &tag, v);
                }
            }
            "ISBN" if r.isbn.is_none() => r.isbn = Some(v.to_string()),
            "PB" => r.publisher = Some(v.to_string()),
            "PL" if r.publisher.is_some() || book_title.is_some() => {
                r.publisher_place = Some(v.to_string());
            }
            "EN" => r.edition = Some(v.to_string()),
            "AID" | "LID" => {
                let lower = v.to_ascii_lowercase();
                if lower.ends_with("[doi]") {
                    if r.doi.is_none() {
                        r.doi = Some(v[..v.len() - 5].trim().to_string());
                    } else if r.doi.as_deref().map(|d| d.to_ascii_lowercase())
                        != Some(lower[..lower.len() - 5].trim().to_string())
                    {
                        r.keep(FORMAT, &tag, v);
                    }
                } else if lower.ends_with("[pmc]") && r.pmcid.is_none() {
                    r.pmcid = Some(v[..v.len() - 5].trim().to_string());
                } else {
                    r.keep(FORMAT, &tag, v);
                }
            }
            _ => r.keep(FORMAT, &tag, v),
        }
    }
    if let Some(n) = pending_full {
        r.author.push(n);
    }
    if let Some(n) = editors_full {
        r.editor.push(n);
    }
    // MEDLINE AU names are "Family Initials"; give them initials with periods.
    for n in &mut r.author {
        if let Some(g) = &n.given {
            if crate::names::is_initials(g) && !g.contains('.') {
                n.given = Some(expand_initials(g));
            }
        }
    }
    match (title, book_title) {
        (Some(t), Some(b)) => {
            r.kind = "chapter".into();
            r.title = Some(t);
            r.container_title = Some(b);
        }
        (Some(t), None) => r.title = Some(t),
        (None, Some(b)) => {
            r.kind = "book".into();
            r.title = Some(b);
        }
        (None, None) => {}
    }
    r.issued = dates.0.or(dates.1);
    r.id = r.pmid.clone().unwrap_or_else(|| (index + 1).to_string());
    finish(&mut r, index, Some(raw.line), diags);
    r
}

/// Read MEDLINE text.
pub fn read(text: &str, diags: &mut Vec<Diagnostic>) -> Vec<Reference> {
    let raws = records(text, diags);
    if raws.is_empty() && !text.trim().is_empty() {
        diags.push(Diagnostic::error(
            "medline.no-records",
            "Found no MEDLINE records (a record starts with PMID- )",
        ));
    }
    raws.into_iter()
        .enumerate()
        .map(|(i, raw)| to_reference(raw, i, diags))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_record() {
        let src = "PMID- 31000002\nOWN - NLM\nTI  - Tidal pools as nurseries: a long title that\n      wraps onto a second line.\nLID - S0000-0000(19)30001-1 [pii]\nLID - 10.5555/tide.2019.2 [doi]\nAB  - Tide pools.\nFAU - Ekdahl, Maja\nAU  - Ekdahl M\nCN  - Coastal Survey Group\nDP  - 2019 Jul-Aug\nJT  - Marine ecology\nTA  - Mar Ecol\nPG  - 220-31\nPT  - Journal Article\nPMC - PMC6000002\nIS  - 0000-0019 (Linking)\n\nPMID- 31000003\nTI  - Second.\nAU  - Smith JA\n";
        let mut d = Vec::new();
        let refs = read(src, &mut d);
        assert_eq!(refs.len(), 2, "{d:?}");
        let r = &refs[0];
        assert_eq!(
            r.title.as_deref(),
            Some("Tidal pools as nurseries: a long title that wraps onto a second line.")
        );
        assert_eq!(r.doi.as_deref(), Some("10.5555/tide.2019.2"));
        assert_eq!(r.author.len(), 2);
        assert_eq!(r.author[0].given.as_deref(), Some("Maja"));
        assert_eq!(r.author[1].literal.as_deref(), Some("Coastal Survey Group"));
        assert_eq!(r.issued.as_ref().unwrap().start.unwrap().month, Some(7));
        assert!(r
            .source_fields
            .iter()
            .any(|f| f.name == "LID" && f.value.ends_with("[pii]")));
        assert_eq!(refs[1].author[0].given.as_deref(), Some("J. A."));
    }

    #[test]
    fn tags() {
        assert_eq!(tag_line("PMID- 1"), Some(("PMID", "1")));
        assert_eq!(tag_line("TI  - x"), Some(("TI", "x")));
        assert_eq!(tag_line("AB  -"), Some(("AB", "")));
        assert_eq!(tag_line("TY  - JOUR"), Some(("TY", "JOUR")));
        assert_eq!(tag_line("      continued"), None);
    }
}
