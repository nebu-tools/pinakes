//! Web of Science exports: "plain text" and "tab-delimited".
//!
//! Plain text starts with `FN` and `VR` lines. Each field is a two-character tag, a space and a
//! value; the value continues on lines indented by three spaces. In the list fields (authors,
//! groups, cited references) each continuation line is a new item; in the others it continues
//! the text. `ER` ends a record and `EF` the file. The tab-delimited export has the same tags
//! as a header row and one record per line, with list items separated by `; `.

use crate::dates::{month_number, parse_date};
use crate::diag::Diagnostic;
use crate::model::{Date, Name, Reference};
use crate::names::parse_name;
use crate::normalise::finish;

pub const FORMAT: &str = "wos";

/// Tags whose continuation lines are separate items.
const LIST_TAGS: &[&str] = &["AU", "AF", "BA", "BF", "BE", "GP", "CA", "CR", "C1", "C3", "RP", "EM", "RI", "OI", "FU", "WC", "SC"];

struct Raw {
    line: usize,
    fields: Vec<(String, Vec<String>)>,
}

fn tag_line(line: &str) -> Option<(&str, &str)> {
    let b = line.as_bytes();
    if b.len() < 2 || !b[0].is_ascii_uppercase() || !(b[1].is_ascii_uppercase() || b[1].is_ascii_digit()) {
        return None;
    }
    match b.get(2) {
        None => Some((&line[..2], "")),
        Some(b' ') => Some((&line[..2], line[3..].trim_end())),
        _ => None,
    }
}

/// Whether text looks like a plain-text export.
pub(crate) fn looks_like_text(text: &str) -> bool {
    let mut lines = text.lines().map(str::trim_end).filter(|l| !l.is_empty());
    matches!(lines.next(), Some(l) if l.starts_with("FN ")) || text.lines().take(3).any(|l| l.starts_with("PT ") ) && text.contains("\nUT ")
}

/// Whether a header row is a tab-delimited export's.
pub(crate) fn is_tab_header(cells: &[String]) -> bool {
    let tags: Vec<&str> = cells.iter().map(|c| c.trim()).collect();
    tags.len() >= 3 && tags.first() == Some(&"PT") && tags.contains(&"TI") && tags.iter().all(|t| t.is_empty() || tag_line(t).is_some())
}

/// Read a plain-text export.
pub fn read_text(text: &str, diags: &mut Vec<Diagnostic>) -> Vec<Reference> {
    let mut raws: Vec<Raw> = Vec::new();
    let mut open: Option<Raw> = None;
    let mut saw_ef = false;
    let mut last_tag_list = false;
    for (n, line) in text.split('\n').enumerate() {
        let n = n + 1;
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("   ") {
            match open.as_mut().and_then(|r| r.fields.last_mut()) {
                Some((_, values)) => {
                    let v = rest.trim();
                    if last_tag_list {
                        values.push(v.to_string());
                    } else if let Some(last) = values.last_mut() {
                        if !last.is_empty() {
                            last.push(' ');
                        }
                        last.push_str(v);
                    }
                }
                None => diags.push(Diagnostic::error("wos.stray-text", "Indented text outside any record; skipped it").at(n)),
            }
            continue;
        }
        let Some((tag, value)) = tag_line(line) else {
            diags.push(Diagnostic::warning("wos.bad-line", "A line is neither a field nor an indented continuation; read it as a continuation").at(n));
            if let Some((_, values)) = open.as_mut().and_then(|r| r.fields.last_mut()) {
                if let Some(last) = values.last_mut() {
                    last.push(' ');
                    last.push_str(line.trim());
                }
            }
            continue;
        };
        match tag {
            "FN" | "VR" if open.is_none() => continue,
            "EF" => {
                if let Some(r) = open.take() {
                    diags.push(Diagnostic::warning("wos.missing-er", "The last record has no ER line").at(r.line));
                    raws.push(r);
                }
                saw_ef = true;
                continue;
            }
            "ER" => {
                match open.take() {
                    Some(r) => raws.push(r),
                    None => diags.push(Diagnostic::info("wos.stray-er", "ER without a record").at(n)),
                }
                continue;
            }
            _ => {}
        }
        if open.is_none() {
            if tag != "PT" {
                diags.push(Diagnostic::warning("wos.no-pt", format!("A record starts with {tag} instead of PT")).at(n));
            }
            open = Some(Raw { line: n, fields: Vec::new() });
        } else if tag == "PT" {
            let r = open.take().unwrap();
            diags.push(Diagnostic::warning("wos.missing-er", "A record has no ER line; the next PT starts a new one").at(r.line));
            raws.push(r);
            open = Some(Raw { line: n, fields: Vec::new() });
        }
        last_tag_list = LIST_TAGS.contains(&tag);
        open.as_mut().unwrap().fields.push((tag.to_string(), vec![value.to_string()]));
    }
    if let Some(r) = open.take() {
        diags.push(Diagnostic::warning("wos.missing-er", "The last record has no ER line").at(r.line));
        raws.push(r);
    }
    if !saw_ef && !raws.is_empty() {
        diags.push(Diagnostic::info("wos.missing-ef", "The file has no EF line; it may be cut short"));
    }
    if raws.is_empty() && !text.trim().is_empty() {
        diags.push(Diagnostic::error("wos.no-records", "Found no Web of Science records (a record starts with PT)"));
    }
    raws.into_iter().enumerate().map(|(i, raw)| to_reference(raw, i, diags)).collect()
}

/// Read a tab-delimited export (already decoded from UTF-16).
pub fn read_tab(text: &str, diags: &mut Vec<Diagnostic>) -> Vec<Reference> {
    let rows = super::csv::rows(text, '\t', false, diags);
    let Some(header) = rows.first() else { return Vec::new() };
    let tags: Vec<String> = header.cells.iter().map(|c| c.trim().to_string()).collect();
    let mut out = Vec::new();
    for (index, row) in rows[1..].iter().enumerate() {
        let mut fields = Vec::new();
        for (i, cell) in row.cells.iter().enumerate() {
            let v = cell.trim();
            if v.is_empty() {
                continue;
            }
            let tag = tags.get(i).cloned().unwrap_or_else(|| format!("column {}", i + 1));
            let values = if LIST_TAGS.contains(&tag.as_str()) { v.split("; ").map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect() } else { vec![v.to_string()] };
            fields.push((tag, values));
        }
        let mut r = to_reference(Raw { line: row.line, fields }, index, diags);
        if row.cells.len() > tags.len() && row.cells[tags.len()..].iter().any(|c| !c.trim().is_empty()) {
            diags.push(Diagnostic::warning("wos.extra-cells", "Row has more cells than the header; kept them as source fields").at(row.line).record(index));
        }
        r.id = r.accession.clone().unwrap_or(r.id);
        out.push(r);
    }
    out
}

fn kind_from_pt_dt(pt: Option<&str>, dt: &[String]) -> &'static str {
    let dt = dt.iter().map(|d| d.to_lowercase()).collect::<Vec<_>>();
    let has = |s: &str| dt.iter().any(|d| d.contains(s));
    if has("proceedings paper") || has("meeting abstract") {
        return "paper-conference";
    }
    if has("book chapter") {
        return "chapter";
    }
    if has("book review") {
        return "review-book";
    }
    if has("data paper") || has("data set") {
        return "dataset";
    }
    if has("preprint") {
        return "article";
    }
    match pt {
        Some("B") => "book",
        Some("S") => "chapter",
        Some("P") => "patent",
        Some("C") => "paper-conference",
        _ => "article-journal",
    }
}

fn to_reference(raw: Raw, index: usize, diags: &mut Vec<Diagnostic>) -> Reference {
    let mut r = Reference::default();
    let mut au = Vec::new();
    let mut af = Vec::new();
    let mut groups = Vec::new();
    let mut corporate = Vec::new();
    let mut be = Vec::new();
    let mut pt = None;
    let mut py = None;
    let mut pd = None;
    let mut ea = None;
    let (mut bp, mut ep, mut ar) = (None, None, None);
    let mut issn = Vec::new();
    for (tag, values) in raw.fields {
        let joined = values.join(" ");
        let v = joined.trim();
        match tag.as_str() {
            "PT" => pt = Some(v.to_string()),
            "AU" => au = values,
            "AF" => af = values,
            "BE" => be = values,
            "GP" => groups = values,
            "CA" => corporate = values,
            "TI" => r.title = Some(v.to_string()),
            "SO" => r.container_title = Some(v.to_string()),
            "SE" | "BS" => r.collection_title = Some(v.to_string()),
            "J9" => {
                r.keep(FORMAT, &tag, v);
            }
            "JI" => r.container_title_short = Some(v.to_string()),
            "DT" => r.publication_types = v.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            "LA" => r.language = Some(v.to_string()),
            "DE" => r.keywords.extend(v.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty())),
            "AB" => r.abstract_text = Some(v.to_string()),
            "PU" => r.publisher = Some(v.to_string()),
            "PI" | "PA" if r.publisher_place.is_none() && tag == "PI" => r.publisher_place = Some(v.to_string()),
            "SN" | "EI" => issn.push(v.to_string()),
            "BN" => r.isbn = Some(v.to_string()),
            "PY" => py = v.parse::<i32>().ok(),
            "PD" => pd = Some(v.to_string()),
            "EA" => ea = Some(v.to_string()),
            "VL" => r.volume = Some(v.to_string()),
            "IS" => r.issue = Some(v.to_string()),
            "BP" => bp = Some(v.to_string()),
            "EP" => ep = Some(v.to_string()),
            "AR" => ar = Some(v.to_string()),
            "DI" => r.doi = Some(v.to_string()),
            "PM" => r.pmid = Some(v.to_string()),
            "UT" => {
                r.accession = Some(v.to_string());
                r.database = Some("Web of Science".into());
            }
            _ => {
                let value = if LIST_TAGS.contains(&tag.as_str()) { values.join("\n") } else { v.to_string() };
                r.keep(FORMAT, &tag, &value);
            }
        }
    }
    // Full names (AF) where the export has them, abbreviated (AU) otherwise.
    let people = if af.len() >= au.len() && !af.is_empty() {
        if !au.is_empty() {
            r.keep(FORMAT, "AU", &au.join("\n"));
        }
        af
    } else {
        if !af.is_empty() {
            r.keep(FORMAT, "AF", &af.join("\n"));
        }
        au
    };
    r.author = people.iter().map(|p| parse_name(p)).filter(|n| !n.is_empty()).collect();
    for g in groups.iter().chain(&corporate) {
        if !g.trim().is_empty() {
            r.author.push(Name::literal(g.trim()));
        }
    }
    r.editor = be.iter().map(|p| parse_name(p)).filter(|n| !n.is_empty()).collect();
    for s in issn {
        if r.issn.is_none() {
            r.issn = Some(s);
        } else {
            r.keep(FORMAT, "EI", &s);
        }
    }
    r.page = match (bp, ep) {
        (Some(b), Some(e)) => Some(format!("{b}-{e}")),
        (Some(b), None) => Some(b),
        _ => ar.clone(),
    };
    if r.page.as_deref() != ar.as_deref() {
        if let Some(a) = ar {
            r.number = Some(a);
        }
    }
    r.issued = match py {
        Some(y) => Some(issued_with_pd(y, pd.as_deref())),
        None => ea.as_deref().and_then(parse_date),
    };
    if py.is_some() {
        if let Some(e) = &ea {
            r.keep(FORMAT, "EA", e);
        }
    }
    if let Some(p) = &pd {
        r.keep(FORMAT, "PD", p);
    }
    r.kind = kind_from_pt_dt(pt.as_deref(), &r.publication_types).to_string();
    if let Some(p) = pt {
        r.keep(FORMAT, "PT", &p);
    }
    r.id = r.accession.clone().unwrap_or_else(|| (index + 1).to_string());
    if r.accession.is_none() {
        diags.push(Diagnostic::warning("wos.no-ut", "A record has no UT accession number").at(raw.line).record(index));
    }
    finish(&mut r, index, Some(raw.line), diags);
    r
}

/// PY with the month and day from PD ("JUL 15", "SUM", "2019 JUL").
fn issued_with_pd(year: i32, pd: Option<&str>) -> Date {
    let mut month = None;
    let mut day = None;
    if let Some(pd) = pd {
        for w in pd.split(|c: char| c.is_whitespace() || c == '-') {
            if month.is_none() {
                month = month_number(w);
            } else if day.is_none() {
                day = w.parse::<u8>().ok().filter(|d| (1..=31).contains(d));
            }
        }
    }
    Date::ymd(year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "FN Clarivate Analytics Web of Science\nVR 1.0\nPT J\nAU Okafor, AN\n   Lind, M\nAF Okafor, Ada N.\n   Lind, Maja\nCA Coastal Survey Team\nTI Moths at the edge of the\n   forest\nSO JOURNAL OF ANIMAL ECOLOGY\nDT Article; Early Access\nPD JUL 15\nPY 2019\nVL 88\nBP 1002\nEP 1013\nDI 10.5555/jae.2019.0001\nPM 31000001\nUT WOS:000000000000001\nER\n\nPT J\nAU Smith, J\nTI Second\nPY 2020\nUT WOS:000000000000002\nER\n\nEF\n";

    #[test]
    fn plain_text() {
        let mut d = Vec::new();
        let refs = read_text(TEXT, &mut d);
        assert_eq!(refs.len(), 2, "{d:?}");
        let r = &refs[0];
        assert_eq!(r.title.as_deref(), Some("Moths at the edge of the forest"));
        assert_eq!(r.author.len(), 3);
        assert_eq!(r.author[0].given.as_deref(), Some("Ada N."));
        assert_eq!(r.author[2].literal.as_deref(), Some("Coastal Survey Team"));
        assert_eq!(r.page.as_deref(), Some("1002-1013"));
        assert_eq!(r.issued.as_ref().unwrap().start.unwrap().month, Some(7));
        assert_eq!(r.id, "WOS:000000000000001");
        assert!(d.iter().all(|x| x.severity != crate::diag::Severity::Error), "{d:?}");
    }

    #[test]
    fn missing_er_and_ef() {
        let mut d = Vec::new();
        let refs = read_text("PT J\nTI One\nUT WOS:1\nPT J\nTI Two\nUT WOS:2\n", &mut d);
        assert_eq!(refs.len(), 2);
        assert!(d.iter().any(|x| x.code == "wos.missing-er"));
        assert!(d.iter().any(|x| x.code == "wos.missing-ef"));
    }

    #[test]
    fn tab_delimited() {
        let src = "PT\tAU\tAF\tTI\tSO\tPY\tDI\tUT\nJ\tLind, M; Okafor, AN\tLind, Maja; Okafor, Ada N.\tTide pools\tMAR ECOL\t2020\t10.5555/y\tWOS:3\n";
        let mut d = Vec::new();
        let refs = read_tab(src, &mut d);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].author.len(), 2);
        assert_eq!(refs[0].id, "WOS:3");
    }
}
