//! Normalising a reference after any reader has filled it.

use crate::diag::Diagnostic;
use crate::id::{self, IdKind};
use crate::model::Reference;

/// Put a reference's identifiers in normal form, find identifiers hiding in its URL or DOI,
/// and tidy page ranges and whitespace. Values that do not validate are kept as they were and
/// reported. Safe to run more than once.
pub fn normalise(r: &mut Reference) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for field in [
        &mut r.title,
        &mut r.title_short,
        &mut r.container_title,
        &mut r.container_title_short,
        &mut r.collection_title,
        &mut r.publisher,
        &mut r.publisher_place,
        &mut r.volume,
        &mut r.issue,
        &mut r.number,
        &mut r.edition,
    ] {
        if let Some(v) = field {
            let squashed = crate::text::squash(v);
            *field = (!squashed.is_empty()).then_some(squashed);
        }
    }
    if let Some(p) = &r.page {
        let fixed = page_range(p);
        r.page = (!fixed.is_empty()).then_some(fixed);
    }

    if r.doi.is_none() {
        if let Some(u) = &r.url {
            if u.to_ascii_lowercase().contains("doi.org/10.") {
                r.doi = id::doi(u);
            }
        }
    }
    if r.arxiv.is_none() {
        let from_doi = r.doi.as_deref().filter(|d| d.starts_with("10.48550/arxiv.")).and_then(id::arxiv);
        let from_url = r.url.as_deref().filter(|u| u.to_ascii_lowercase().contains("arxiv.org/")).and_then(id::arxiv);
        r.arxiv = from_doi.or(from_url);
    }

    let mut check = |kind: IdKind, slot: &mut Option<String>| {
        let Some(raw) = slot.clone() else { return };
        if raw.trim().is_empty() {
            *slot = None;
            return;
        }
        match id::normalise(kind, &raw) {
            Ok(v) => *slot = Some(v),
            Err(e) => {
                // ISBN and ISSN fields often hold several values; normalise each.
                if matches!(kind, IdKind::Isbn | IdKind::Issn) {
                    let parts: Vec<&str> = raw.split([';', ',', ' ', '\n']).filter(|s| !s.trim().is_empty()).collect();
                    let good: Vec<String> = parts.iter().filter_map(|p| id::normalise(kind, p).ok()).collect();
                    if !good.is_empty() && parts.len() > 1 {
                        *slot = Some(good.join(" "));
                        return;
                    }
                }
                diags.push(Diagnostic::warning(code_for(kind), format!("{e}; kept it as written")));
                *slot = Some(raw.trim().to_string());
            }
        }
    };
    check(IdKind::Doi, &mut r.doi);
    check(IdKind::Pmid, &mut r.pmid);
    check(IdKind::Pmcid, &mut r.pmcid);
    check(IdKind::Isbn, &mut r.isbn);
    check(IdKind::Issn, &mut r.issn);
    check(IdKind::Arxiv, &mut r.arxiv);

    r.author.retain(|n| !n.is_empty());
    r.editor.retain(|n| !n.is_empty());
    let mut seen = std::collections::HashSet::new();
    r.keywords.retain(|k| !k.trim().is_empty() && seen.insert(k.trim().to_lowercase()));
    diags
}

fn code_for(kind: IdKind) -> &'static str {
    match kind {
        IdKind::Doi => "id.invalid-doi",
        IdKind::Pmid => "id.invalid-pmid",
        IdKind::Pmcid => "id.invalid-pmcid",
        IdKind::Isbn => "id.invalid-isbn",
        IdKind::Issn => "id.invalid-issn",
        IdKind::Arxiv => "id.invalid-arxiv",
    }
}

/// "301--310", "301 – 310", "301—10" become "301-310" and "301-10". Other text is kept.
pub fn page_range(p: &str) -> String {
    let p = p.trim();
    let mut out = String::with_capacity(p.len());
    let mut chars = p.chars().peekable();
    while let Some(c) = chars.next() {
        if matches!(c, '-' | '\u{2010}'..='\u{2015}' | '\u{2212}') {
            while out.ends_with(' ') {
                out.pop();
            }
            while matches!(chars.peek(), Some('-' | '\u{2010}'..='\u{2015}' | '\u{2212}' | ' ')) {
                chars.next();
            }
            out.push('-');
        } else {
            out.push(c);
        }
    }
    out
}

/// Finish a freshly read record: normalise it, attach diagnostics to it, and note a missing
/// title.
pub(crate) fn finish(r: &mut Reference, index: usize, line: Option<usize>, diags: &mut Vec<Diagnostic>) {
    for mut d in normalise(r) {
        d.record = Some(index);
        d.line = d.line.or(line);
        diags.push(d);
    }
    if r.title.is_none() {
        let mut d = Diagnostic::warning("record.no-title", format!("Record {} has no title", r.id)).record(index);
        d.line = line;
        diags.push(d);
    }
    if r.kind.is_empty() {
        r.kind = "document".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages() {
        assert_eq!(page_range("301--310"), "301-310");
        assert_eq!(page_range("301 – 310"), "301-310");
        assert_eq!(page_range("e1234"), "e1234");
        assert_eq!(page_range("12-14, 18"), "12-14, 18");
    }
}
