//! RIS, in the dialects EndNote, Zotero, Mendeley, PubMed-derived tools, Ovid (MEDLINE and
//! Embase), Web of Science, Scopus and the Cochrane Library write.
//!
//! The reader is lenient in the ways exports differ: one or two spaces before the dash, no
//! space after it, `ER` with or without a trailing space or newline, no blank line between
//! records, wrapped values on lines without a tag, four-letter tags such as `PMID`. A record
//! without `ER` is kept and reported. Tags Pinakes does not map are kept as source fields and
//! written back by [`write`].

use crate::diag::Diagnostic;
use crate::model::{Date, Reference};
use crate::names::parse_name;
use crate::normalise::finish;
use crate::text::non_empty;

pub(crate) const FORMAT: &str = "ris";

/// A tag line: `TY  - JOUR`. Returns the tag and the value with trailing space removed.
pub(crate) fn tag_line(line: &str) -> Option<(&str, &str)> {
    let b = line.as_bytes();
    if b.is_empty() || !b[0].is_ascii_uppercase() {
        return None;
    }
    let n = b.iter().take(5).take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit()).count();
    if !(2..=4).contains(&n) {
        return None;
    }
    let spaces = b[n..].iter().take_while(|c| **c == b' ').count();
    let dash = n + spaces;
    if b.get(dash) != Some(&b'-') {
        return None;
    }
    if !(1..=3).contains(&spaces) && !(spaces == 0 && n == 4) {
        return None;
    }
    let rest = &line[dash + 1..];
    let value = match rest.as_bytes().first() {
        None => "",
        Some(b' ') => &rest[1..],
        // "TY  -JOUR": tolerated only with the standard two spaces before the dash.
        Some(_) if spaces == 2 && n == 2 => rest,
        Some(_) => return None,
    };
    Some((&line[..n], value.trim_end()))
}

#[derive(Debug)]
struct RawRecord {
    line: usize,
    fields: Vec<(String, String, usize)>,
    closed: bool,
}

fn joins_with_newline(tag: &str) -> bool {
    matches!(tag, "AB" | "N1" | "N2")
}

/// Split RIS text into records of (tag, value, line).
fn records(text: &str, diags: &mut Vec<Diagnostic>) -> Vec<RawRecord> {
    let mut out: Vec<RawRecord> = Vec::new();
    let mut current: Option<RawRecord> = None;
    let mut blank_pending = false;
    let mut stray: Vec<(usize, String)> = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        let lineno = i + 1;
        if line.trim().is_empty() {
            blank_pending = true;
            continue;
        }
        if let Some((tag, value)) = tag_line(line) {
            let tag = tag.to_string();
            blank_pending = false;
            if tag == "TY" {
                if let Some(rec) = current.take() {
                    diags.push(
                        Diagnostic::warning("ris.missing-er", format!("Record starting on line {} has no ER; it ends where the next TY starts", rec.line))
                            .at(rec.line)
                            .record(out.len()),
                    );
                    out.push(rec);
                }
                current = Some(RawRecord { line: lineno, fields: vec![(tag, value.to_string(), lineno)], closed: false });
                continue;
            }
            if tag == "ER" {
                match current.take() {
                    Some(mut rec) => {
                        rec.closed = true;
                        out.push(rec);
                    }
                    None => diags.push(Diagnostic::warning("ris.stray-er", "ER with no record open; ignored it").at(lineno)),
                }
                continue;
            }
            let rec = current.get_or_insert_with(|| {
                diags.push(
                    Diagnostic::warning("ris.no-ty", format!("{tag} before any TY; started a record of unknown type"))
                        .at(lineno)
                        .record(out.len()),
                );
                RawRecord { line: lineno, fields: Vec::new(), closed: false }
            });
            rec.fields.push((tag, value.to_string(), lineno));
            continue;
        }
        // A line without a tag continues the previous value.
        match current.as_mut().and_then(|r| r.fields.last_mut()) {
            Some((tag, value, _)) => {
                let piece = line.trim();
                let piece = piece.strip_prefix("- ").unwrap_or(piece);
                if !value.is_empty() {
                    if joins_with_newline(tag) && (blank_pending || line.starts_with(|c: char| !c.is_whitespace())) {
                        value.push('\n');
                    } else {
                        value.push(' ');
                    }
                }
                value.push_str(piece);
            }
            None => stray.push((lineno, line.trim().to_string())),
        }
        blank_pending = false;
    }
    if let Some(rec) = current.take() {
        diags.push(
            Diagnostic::warning("ris.missing-er", format!("Record starting on line {} has no ER at the end of the input; kept it", rec.line))
                .at(rec.line)
                .record(out.len()),
        );
        out.push(rec);
    }
    for (line, text) in stray {
        let shown: String = text.chars().take(60).collect();
        diags.push(Diagnostic::error("ris.stray-text", format!("Text outside any record was not read: {shown:?}")).at(line));
    }
    out
}

/// RIS type to CSL type.
pub fn kind_from_ris(ty: &str) -> &'static str {
    match ty.trim().to_ascii_uppercase().as_str() {
        "JOUR" | "EJOUR" | "JFULL" | "ABST" | "INPR" => "article-journal",
        "MGZN" => "article-magazine",
        "NEWS" => "article-newspaper",
        "BOOK" | "EBOOK" | "EDBOOK" | "WHOLE" | "SER" | "CLSWK" | "ANCIENT" => "book",
        "CHAP" | "ECHAP" => "chapter",
        "CONF" | "CPAPER" => "paper-conference",
        "RPRT" | "GOVDOC" => "report",
        "THES" => "thesis",
        "ELEC" | "WEB" | "ICOMM" => "webpage",
        "BLOG" => "post-weblog",
        "DATA" | "DBASE" | "AGGR" => "dataset",
        "COMP" => "software",
        "PAT" => "patent",
        "STAT" | "LEGAL" => "legislation",
        "BILL" | "UNBILL" => "bill",
        "CASE" => "legal_case",
        "HEAR" => "hearing",
        "STAND" => "standard",
        "MAP" => "map",
        "ART" | "SLIDE" | "FIGURE" | "EQUA" | "CHART" => "graphic",
        "SOUND" | "MUSIC" => "song",
        "VIDEO" | "MPCT" | "ADVS" => "motion_picture",
        "PCOMM" => "personal_communication",
        "MANSCPT" => "manuscript",
        "UNPB" => "article",
        "PAMP" => "pamphlet",
        "DICT" => "entry-dictionary",
        "ENCYC" => "entry-encyclopedia",
        _ => "document",
    }
}

/// CSL type to RIS type.
pub fn kind_to_ris(kind: &str) -> &'static str {
    match kind {
        "article-journal" | "review" | "review-book" => "JOUR",
        "article-magazine" => "MGZN",
        "article-newspaper" => "NEWS",
        "book" | "classic" => "BOOK",
        "chapter" | "entry" => "CHAP",
        "paper-conference" => "CPAPER",
        "report" => "RPRT",
        "thesis" => "THES",
        "webpage" => "ELEC",
        "post-weblog" => "BLOG",
        "post" => "ELEC",
        "dataset" => "DATA",
        "software" => "COMP",
        "patent" => "PAT",
        "legislation" | "regulation" | "treaty" => "STAT",
        "bill" => "BILL",
        "legal_case" => "CASE",
        "hearing" => "HEAR",
        "standard" => "STAND",
        "map" => "MAP",
        "graphic" | "figure" => "ART",
        "song" => "SOUND",
        "motion_picture" | "broadcast" => "MPCT",
        "personal_communication" | "interview" | "letter" => "PCOMM",
        "manuscript" => "MANSCPT",
        "article" => "UNPB",
        "pamphlet" => "PAMP",
        "entry-dictionary" => "DICT",
        "entry-encyclopedia" => "ENCYC",
        _ => "GEN",
    }
}

fn is_bookish(kind: &str) -> bool {
    matches!(kind, "book" | "chapter")
}

/// What the record's database fields say about where it came from.
fn pubmed_hint(fields: &[(String, String, usize)]) -> Option<bool> {
    let mut verdict = None;
    for (tag, value, _) in fields {
        if !matches!(tag.as_str(), "DB" | "DP" | "UR" | "N1" | "SO" | "L2") {
            continue;
        }
        let v = value.to_ascii_lowercase();
        if ["embase", "scopus", "web of science", "webofscience", "cinahl", "psycinfo", "ebsco", "proquest", "cochrane"].iter().any(|k| v.contains(k)) {
            return Some(false);
        }
        if ["pubmed", "medline", "ncbi.nlm.nih.gov"].iter().any(|k| v.contains(k)) {
            verdict = Some(true);
        }
    }
    verdict
}

fn looks_like_database(s: &str) -> bool {
    let v = s.to_ascii_lowercase();
    ["cochrane", "register", "database", "library", "ovid", "embase", "medline", "pubmed", "scopus", "web of science"]
        .iter()
        .any(|k| v.contains(k))
        && !v.contains(',')
}

fn set_once(slot: &mut Option<String>, value: &str) -> bool {
    if slot.is_none() {
        *slot = non_empty(value);
        true
    } else {
        false
    }
}

fn to_reference(raw: RawRecord, index: usize, diags: &mut Vec<Diagnostic>) -> Reference {
    let mut r = Reference::default();
    let ty = raw.fields.iter().find(|(t, _, _)| t == "TY").map(|(_, v, _)| v.trim().to_string());
    match &ty {
        Some(t) => {
            r.kind = kind_from_ris(t).to_string();
            if kind_to_ris(&r.kind) != t.to_ascii_uppercase() {
                r.keep(FORMAT, "TY", t);
            }
            if r.kind == "document" && !t.eq_ignore_ascii_case("GEN") {
                diags.push(
                    Diagnostic::info("ris.unknown-type", format!("RIS type {t:?} is not one Pinakes knows; read it as a generic document"))
                        .at(raw.line)
                        .record(index),
                );
            }
        }
        None => r.kind = "document".into(),
    }
    let bookish = is_bookish(&r.kind);
    let hint = pubmed_hint(&raw.fields);
    let (mut sp, mut ep) = (None::<String>, None::<String>);
    let (mut py, mut da) = (None::<Date>, None::<Date>);
    let mut notes: Vec<String> = Vec::new();
    let mut ris_id = None;
    let mut journal_full = None;
    let mut journal_other = None;
    for (tag, value, line) in &raw.fields {
        let v = value.as_str();
        if v.trim().is_empty() {
            if tag != "TY" {
                r.keep(FORMAT, tag, v);
            }
            continue;
        }
        let kept = match tag.as_str() {
            "TY" => true,
            "ID" => set_once(&mut ris_id, v),
            "TI" | "T1" | "CT" => set_once(&mut r.title, v),
            "BT" => {
                if bookish && r.kind == "book" {
                    set_once(&mut r.title, v)
                } else {
                    set_once(&mut r.container_title, v)
                }
            }
            "T2" => {
                if r.kind == "book" {
                    set_once(&mut r.collection_title, v)
                } else {
                    set_once(&mut r.container_title, v)
                }
            }
            "T3" => set_once(&mut r.collection_title, v),
            "JF" => set_once(&mut journal_full, v),
            "JO" => set_once(&mut journal_other, v),
            "JA" | "J1" | "J2" => set_once(&mut r.container_title_short, v),
            "ST" => set_once(&mut r.title_short, v),
            "AU" | "A1" => {
                r.author.push(parse_name(v));
                true
            }
            "A2" | "ED" => {
                r.editor.push(parse_name(v));
                true
            }
            "PY" | "Y1" => {
                if py.is_none() {
                    py = crate::dates::parse_date(v);
                    py.is_some()
                } else {
                    false
                }
            }
            "DA" => {
                if da.is_none() {
                    da = crate::dates::parse_date(v);
                    da.is_some()
                } else {
                    false
                }
            }
            "Y2" => {
                if r.accessed.is_none() {
                    r.accessed = crate::dates::parse_date(v);
                    r.accessed.is_some()
                } else {
                    false
                }
            }
            "SP" => set_once(&mut sp, v),
            "EP" => set_once(&mut ep, v),
            "VL" => set_once(&mut r.volume, v),
            "IS" | "CP" => set_once(&mut r.issue, v),
            "ET" => set_once(&mut r.edition, v),
            "PB" => set_once(&mut r.publisher, v),
            "CY" | "PP" => set_once(&mut r.publisher_place, v),
            "SN" => {
                if crate::id::isbn(v).is_ok() || (bookish && crate::id::issn(v).is_err()) {
                    set_once(&mut r.isbn, v)
                } else {
                    set_once(&mut r.issn, v)
                }
            }
            "DO" => set_once(&mut r.doi, v),
            "UR" | "L2" => {
                if r.doi.is_none() && v.to_ascii_lowercase().contains("doi.org/10.") {
                    r.doi = crate::id::doi(v);
                    // Keep the URL too: some readers want the link.
                }
                set_once(&mut r.url, v)
            }
            "AN" => {
                let first = set_once(&mut r.accession, v);
                let digits = v.trim();
                if first
                    && r.pmid.is_none()
                    && hint != Some(false)
                    && digits.len() <= 8
                    && digits.chars().all(|c| c.is_ascii_digit())
                {
                    r.pmid = Some(digits.to_string());
                }
                first
            }
            "PMID" => {
                r.pmid = non_empty(v);
                true
            }
            "C2" if v.trim().to_ascii_uppercase().starts_with("PMC") && r.pmcid.is_none() => {
                r.pmcid = non_empty(v);
                false
            }
            "M3" if r.pmid.is_none() && v.trim().chars().all(|c| c.is_ascii_digit()) && v.trim().len() <= 8 && hint != Some(false) => {
                r.pmid = non_empty(v);
                false
            }
            "AB" => set_once(&mut r.abstract_text, v),
            "N2" => set_once(&mut r.abstract_text, v),
            "N1" => {
                notes.push(v.trim().to_string());
                true
            }
            "KW" => {
                for k in v.split(['\n', ';']) {
                    if let Some(k) = non_empty(k) {
                        r.keywords.push(k);
                    }
                }
                true
            }
            "LA" => set_once(&mut r.language, v),
            "DB" | "DP" => set_once(&mut r.database, v),
            "SO" => {
                if looks_like_database(v) {
                    set_once(&mut r.database, v)
                } else {
                    set_once(&mut journal_other, v)
                }
            }
            "M1" if r.kind == "report" => set_once(&mut r.number, v),
            "M3" if r.genre.is_none() && r.kind == "thesis" => set_once(&mut r.genre, v),
            _ => false,
        };
        if !kept {
            r.keep(FORMAT, tag, v);
        }
        let _ = line;
    }
    if r.container_title.is_none() {
        r.container_title = journal_full.clone().or_else(|| journal_other.clone());
    } else {
        if let Some(j) = journal_full.filter(|j| Some(j) != r.container_title.as_ref()) {
            r.keep(FORMAT, "JF", &j);
        }
        if let Some(j) = journal_other.filter(|j| Some(j) != r.container_title.as_ref()) {
            if r.container_title_short.is_none() {
                r.container_title_short = Some(j);
            } else {
                r.keep(FORMAT, "JO", &j);
            }
        }
    }
    r.page = match (sp, ep) {
        (Some(s), Some(e)) if !s.contains(['-', '\u{2013}']) => Some(format!("{s}-{e}")),
        (Some(s), _) => Some(s),
        (None, Some(e)) => Some(e),
        (None, None) => None,
    };
    // DA is the fuller date in EndNote and Zotero exports; prefer it when it agrees with PY.
    r.issued = match (py, da) {
        (Some(p), Some(d)) => {
            let same_year = p.start.map(|x| x.year) == d.start.map(|x| x.year);
            if same_year && d.start.is_some_and(|x| x.month.is_some()) {
                Some(d)
            } else {
                r.keep(FORMAT, "DA", &d.iso().unwrap_or_default());
                Some(p)
            }
        }
        (p, d) => p.or(d),
    };
    if !notes.is_empty() {
        r.note = Some(notes.join("\n"));
    }
    r.id = ris_id.or_else(|| r.accession.clone()).unwrap_or_else(|| (index + 1).to_string());
    finish(&mut r, index, Some(raw.line), diags);
    r
}

/// Read RIS text.
pub fn read(text: &str, diags: &mut Vec<Diagnostic>) -> Vec<Reference> {
    let raws = records(text, diags);
    if raws.is_empty() && !text.trim().is_empty() {
        diags.push(Diagnostic::error("ris.no-records", "Found no RIS records (a record starts with a TY line)"));
    }
    raws.into_iter().enumerate().map(|(i, raw)| to_reference(raw, i, diags)).collect()
}

fn push(out: &mut String, tag: &str, value: &str) {
    let mut lines = value.split('\n');
    let first = lines.next().unwrap_or("");
    out.push_str(tag);
    out.push_str("  - ");
    out.push_str(first.trim_end());
    out.push('\n');
    for more in lines {
        let more = more.trim();
        if !more.is_empty() {
            out.push_str(more);
            out.push('\n');
        }
    }
}

fn one_line(s: &str) -> String {
    crate::text::squash(s)
}

/// Write RIS. Each record ends with `ER  - ` and a blank line. Values that held line breaks
/// (abstracts, notes) continue on lines without a tag, which [`read`] joins back.
pub fn write(refs: &[Reference]) -> String {
    let mut out = String::new();
    for r in refs {
        let ty = r
            .source_fields_of(FORMAT)
            .find(|f| f.name == "TY")
            .map(|f| f.value.clone())
            .unwrap_or_else(|| kind_to_ris(&r.kind).to_string());
        push(&mut out, "TY", &ty);
        if let Some(k) = r.citation_key.as_ref().or(Some(&r.id)).filter(|k| !k.is_empty()) {
            if r.accession.as_deref() != Some(k.as_str()) || r.citation_key.is_some() {
                push(&mut out, "ID", k);
            }
        }
        if let Some(t) = &r.title {
            push(&mut out, "TI", &one_line(t));
        }
        if let Some(t) = &r.title_short {
            push(&mut out, "ST", &one_line(t));
        }
        for a in &r.author {
            push(&mut out, "AU", &a.inverted());
        }
        for e in &r.editor {
            push(&mut out, "A2", &e.inverted());
        }
        if let Some(d) = &r.issued {
            match (&d.start, &d.literal) {
                (Some(s), _) => {
                    push(&mut out, "PY", &s.year.to_string());
                    if s.month.is_some() {
                        push(&mut out, "DA", &ris_date(s));
                    }
                }
                (None, Some(l)) => push(&mut out, "PY", l),
                _ => {}
            }
        }
        if let Some(d) = r.accessed.as_ref().and_then(|d| d.start) {
            push(&mut out, "Y2", &ris_date(&d));
        }
        if let Some(c) = &r.container_title {
            push(&mut out, "T2", &one_line(c));
        }
        if let Some(c) = &r.container_title_short {
            push(&mut out, "J2", &one_line(c));
        }
        if let Some(c) = &r.collection_title {
            push(&mut out, if r.kind == "book" { "T2" } else { "T3" }, &one_line(c));
        }
        for (tag, v) in [("VL", &r.volume), ("IS", &r.issue), ("ET", &r.edition), ("PB", &r.publisher), ("CY", &r.publisher_place)] {
            if let Some(v) = v {
                push(&mut out, tag, &one_line(v));
            }
        }
        if let Some(n) = &r.number {
            push(&mut out, "M1", n);
        }
        if let Some(p) = &r.page {
            match p.split_once('-') {
                Some((s, e)) if !p.contains(',') => {
                    push(&mut out, "SP", s.trim());
                    push(&mut out, "EP", e.trim());
                }
                _ => push(&mut out, "SP", p),
            }
        }
        if let Some(v) = r.isbn.as_ref().or(r.issn.as_ref()) {
            push(&mut out, "SN", v);
        }
        if let (Some(_), Some(issn)) = (&r.isbn, &r.issn) {
            push(&mut out, "SN", issn);
        }
        if let Some(d) = &r.doi {
            push(&mut out, "DO", d);
        }
        if let Some(u) = &r.url {
            push(&mut out, "UR", u);
        }
        match (&r.accession, &r.pmid) {
            (Some(a), _) => push(&mut out, "AN", a),
            (None, Some(p)) => push(&mut out, "AN", p),
            _ => {}
        }
        if let (Some(a), Some(p)) = (&r.accession, &r.pmid) {
            if a != p {
                push(&mut out, "PMID", p);
            }
        }
        if let Some(p) = &r.pmcid {
            push(&mut out, "C2", p);
        }
        if let Some(a) = &r.abstract_text {
            push(&mut out, "AB", a);
        }
        if let Some(n) = &r.note {
            push(&mut out, "N1", n);
        }
        for k in &r.keywords {
            push(&mut out, "KW", k);
        }
        if let Some(l) = &r.language {
            push(&mut out, "LA", l);
        }
        if let Some(d) = &r.database {
            push(&mut out, "DB", d);
        }
        if let Some(g) = &r.genre {
            push(&mut out, "M3", g);
        }
        for f in r.source_fields_of(FORMAT) {
            if f.name != "TY" && f.name != "ER" {
                push(&mut out, &f.name, &f.value);
            }
        }
        out.push_str("ER  - \n\n");
    }
    out
}

fn ris_date(d: &crate::model::Ymd) -> String {
    let m = d.month.map(|m| format!("{m:02}")).unwrap_or_default();
    let day = d.day.map(|x| format!("{x:02}")).unwrap_or_default();
    format!("{:04}/{m}/{day}/", d.year)
}


#[cfg(test)]
mod tests {
    use super::*;

    fn read_str(s: &str) -> (Vec<Reference>, Vec<Diagnostic>) {
        let mut d = Vec::new();
        let r = read(s, &mut d);
        (r, d)
    }

    #[test]
    fn tag_lines() {
        assert_eq!(tag_line("TY  - JOUR"), Some(("TY", "JOUR")));
        assert_eq!(tag_line("ER  - "), Some(("ER", "")));
        assert_eq!(tag_line("ER  -"), Some(("ER", "")));
        assert_eq!(tag_line("PMID- 123"), Some(("PMID", "123")));
        assert_eq!(tag_line("AU - Smith, J"), Some(("AU", "Smith, J")));
        assert_eq!(tag_line("ABC-testing"), None);
        assert_eq!(tag_line("  with more text"), None);
        assert_eq!(tag_line("Ab  - x"), None);
    }

    #[test]
    fn unknown_tags_survive_a_round_trip() {
        let (refs, _) = read_str("TY  - JOUR\nTI  - Moths\nQZ  - keep me\nQZ  - and me\nER  - \n");
        let out = write(&refs);
        assert!(out.contains("QZ  - keep me\nQZ  - and me\n"), "{out}");
        let (again, _) = read_str(&out);
        assert_eq!(again, refs);
    }

    #[test]
    fn truncated_record_is_kept_and_reported() {
        let (refs, diags) = read_str("TY  - JOUR\nTI  - Complete\nER  - \nTY  - JOUR\nTI  - Cut short\n");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[1].title.as_deref(), Some("Cut short"));
        assert!(diags.iter().any(|d| d.code == "ris.missing-er" && d.line == Some(4)));
    }

    #[test]
    fn continuation_lines() {
        let (refs, _) = read_str("TY  - JOUR\nTI  - A long title\n  that wraps\nAB  - First paragraph.\nSecond paragraph.\nER  - ");
        assert_eq!(refs[0].title.as_deref(), Some("A long title that wraps"));
        assert_eq!(refs[0].abstract_text.as_deref(), Some("First paragraph.\nSecond paragraph."));
    }

    #[test]
    fn orphan_text_is_reported() {
        let (refs, diags) = read_str("stray words\nTY  - JOUR\nTI  - x\nER  - \nmore stray\n");
        assert_eq!(refs.len(), 1);
        assert_eq!(diags.iter().filter(|d| d.code == "ris.stray-text").count(), 2);
    }

    #[test]
    fn accession_numbers() {
        let (refs, _) = read_str("TY  - JOUR\nTI  - x\nAN  - 31987691\nER  - \nTY  - JOUR\nTI  - y\nAN  - 615024109\nUR  - https://www.embase.com/records/615024109\nER  - \nTY  - JOUR\nTI  - z\nAN  - 2002345\nDB  - Embase\nER  - ");
        assert_eq!(refs[0].pmid.as_deref(), Some("31987691"));
        assert_eq!(refs[1].pmid, None);
        assert_eq!(refs[1].accession.as_deref(), Some("615024109"));
        assert_eq!(refs[2].pmid, None);
    }
}
