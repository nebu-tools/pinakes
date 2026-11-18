//! BibTeX and BibLaTeX.
//!
//! The reader takes both dialects: `@string` macros with `#` concatenation, the month macros,
//! `@comment` and `@preamble`, braced, quoted and bare values, and either brace or parenthesis
//! delimiters. Text between entries is a comment in BibTeX and is skipped. A broken entry is
//! kept with the fields read before the break, and reading resumes at the next line that starts
//! with `@`.
//!
//! Fields with no CSL home (`file`, `owner`, `timestamp`, `crossref`, ...) are kept as source
//! fields and written back by the BibTeX and BibLaTeX writers.

use std::collections::HashMap;

use crate::dates::{month_number, parse_date};
use crate::diag::Diagnostic;
use crate::formats::latex::{latex_to_text, text_to_latex};
use crate::model::{Date, Name, Reference};
use crate::names::{given_family, looks_corporate};
use crate::normalise::finish;
use crate::text::LineIndex;

pub const FORMAT: &str = "bibtex";

/// A raw entry: type, key, fields in source order with the line each started on.
struct Entry {
    kind: String,
    key: String,
    line: usize,
    fields: Vec<(String, String, usize)>,
}

struct Lexer<'a> {
    src: &'a str,
    b: &'a [u8],
    i: usize,
    lines: LineIndex,
    strings: HashMap<String, String>,
}

#[derive(Debug)]
struct Broken(&'static str, &'static str, usize);

impl<'a> Lexer<'a> {
    fn line(&self, at: usize) -> usize {
        self.lines.line(at)
    }

    fn ws(&mut self) {
        while self.i < self.b.len() {
            match self.b[self.i] {
                b' ' | b'\t' | b'\n' | b'\r' => self.i += 1,
                // A % comment inside an entry runs to the end of the line.
                b'%' => {
                    while self.i < self.b.len() && self.b[self.i] != b'\n' {
                        self.i += 1;
                    }
                }
                _ => break,
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    /// A balanced group starting at an opening delimiter; returns the inside.
    fn balanced(&mut self, open: u8, close: u8) -> Result<&'a str, Broken> {
        let start = self.i;
        let mut depth = 0usize;
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if c == b'\\' {
                self.i += 2;
                continue;
            }
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    self.i += 1;
                    return Ok(&self.src[start + 1..self.i - 1]);
                }
            } else if c == b'@'
                && open == b'{'
                && depth <= 1
                && self.at_line_start(self.i)
                && self.looks_like_entry(self.i)
            {
                break;
            }
            self.i += 1;
        }
        self.i = self.i.min(self.b.len());
        Err(Broken("bibtex.unclosed", "a brace is not closed", start))
    }

    fn at_line_start(&self, at: usize) -> bool {
        at == 0 || self.b[at - 1] == b'\n'
    }

    fn looks_like_entry(&self, at: usize) -> bool {
        let rest = &self.b[at + 1..];
        let n = rest.iter().take_while(|c| c.is_ascii_alphabetic()).count();
        n > 0
            && rest[n..]
                .iter()
                .find(|c| !c.is_ascii_whitespace())
                .is_some_and(|c| *c == b'{' || *c == b'(')
    }

    fn quoted(&mut self) -> Result<&'a str, Broken> {
        let start = self.i;
        self.i += 1;
        let mut depth = 0usize;
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'\\' => self.i += 1,
                b'{' => depth += 1,
                b'}' => depth = depth.saturating_sub(1),
                b'"' if depth == 0 => {
                    self.i += 1;
                    return Ok(&self.src[start + 1..self.i - 1]);
                }
                b'@' if self.at_line_start(self.i) && self.looks_like_entry(self.i) => break,
                _ => {}
            }
            self.i += 1;
        }
        self.i = self.i.min(self.b.len());
        Err(Broken(
            "bibtex.unclosed-quote",
            "a quoted value is not closed",
            start,
        ))
    }

    fn bare(&mut self) -> &'a str {
        let start = self.i;
        while self.i < self.b.len()
            && !matches!(
                self.b[self.i],
                b' ' | b'\t' | b'\n' | b'\r' | b',' | b'#' | b'}' | b')' | b'=' | b'"' | b'{'
            )
        {
            self.i += 1;
        }
        &self.src[start..self.i]
    }

    /// A value: parts joined by `#`.
    fn value(&mut self, diags: &mut Vec<Diagnostic>) -> Result<String, Broken> {
        let mut out = String::new();
        loop {
            self.ws();
            match self.peek() {
                Some(b'{') => out.push_str(self.balanced(b'{', b'}')?),
                Some(b'"') => out.push_str(self.quoted()?),
                Some(_) => {
                    let at = self.i;
                    let word = self.bare();
                    if word.is_empty() {
                        return Err(Broken("bibtex.no-value", "a field has no value", at));
                    }
                    if word.bytes().all(|b| b.is_ascii_digit()) {
                        out.push_str(word);
                    } else if let Some(v) = self.strings.get(&word.to_ascii_lowercase()) {
                        out.push_str(v);
                    } else if let Some(m) = month_macro(word) {
                        out.push_str(m);
                    } else {
                        diags.push(
                            Diagnostic::warning(
                                "bibtex.unknown-macro",
                                format!("@string macro {word:?} is not defined; kept its name"),
                            )
                            .at(self.line(at)),
                        );
                        out.push_str(word);
                    }
                }
                None => {
                    return Err(Broken(
                        "bibtex.no-value",
                        "the file ends inside a field",
                        self.i,
                    ))
                }
            }
            self.ws();
            if self.peek() == Some(b'#') {
                self.i += 1;
            } else {
                return Ok(out);
            }
        }
    }

    fn name(&mut self) -> &'a str {
        let start = self.i;
        while self.i < self.b.len()
            && !matches!(
                self.b[self.i],
                b' ' | b'\t'
                    | b'\n'
                    | b'\r'
                    | b','
                    | b'='
                    | b'{'
                    | b'}'
                    | b'('
                    | b')'
                    | b'"'
                    | b'#'
                    | b'%'
            )
        {
            self.i += 1;
        }
        &self.src[start..self.i]
    }

    /// Skip to the next line that starts with `@`.
    fn resync(&mut self) {
        while self.i < self.b.len() {
            if self.b[self.i] == b'@' && self.at_line_start(self.i) {
                return;
            }
            self.i += 1;
        }
    }

    fn entries(&mut self, diags: &mut Vec<Diagnostic>) -> Vec<Entry> {
        let mut out = Vec::new();
        let mut outside_from: Option<usize> = None;
        while self.i < self.b.len() {
            if self.b[self.i] != b'@' {
                if !self.b[self.i].is_ascii_whitespace() && outside_from.is_none() {
                    outside_from = Some(self.i);
                }
                self.i += 1;
                continue;
            }
            if let Some(at) = outside_from.take() {
                let text = self.src[at..self.i].trim();
                if looks_like_lost_fields(text) {
                    diags.push(
                        Diagnostic::warning("bibtex.outside-text", "Text outside any entry looks like fields; BibTeX ignores it and so did Pinakes")
                            .at(self.line(at)),
                    );
                }
            }
            let at = self.i;
            self.i += 1;
            let kind = self.name().to_ascii_lowercase();
            self.ws();
            let open = match self.peek() {
                Some(c @ (b'{' | b'(')) => c,
                _ => {
                    // "@" in running text (an e-mail address in a comment).
                    continue;
                }
            };
            let close = if open == b'{' { b'}' } else { b')' };
            match kind.as_str() {
                "comment" => {
                    if self.balanced(open, close).is_err() {
                        diags.push(
                            Diagnostic::warning("bibtex.unclosed", "An @comment is not closed")
                                .at(self.line(at)),
                        );
                        self.resync();
                    }
                    continue;
                }
                "preamble" => {
                    match self.balanced(open, close) {
                        Ok(_) => diags.push(
                            Diagnostic::info("bibtex.preamble", "Skipped an @preamble")
                                .at(self.line(at)),
                        ),
                        Err(_) => {
                            diags.push(
                                Diagnostic::warning(
                                    "bibtex.unclosed",
                                    "An @preamble is not closed",
                                )
                                .at(self.line(at)),
                            );
                            self.resync();
                        }
                    }
                    continue;
                }
                "string" => {
                    self.i += 1;
                    self.ws();
                    let name = self.name().to_ascii_lowercase();
                    self.ws();
                    if self.peek() == Some(b'=') {
                        self.i += 1;
                    }
                    match self.value(diags) {
                        Ok(v) => {
                            self.strings.insert(name, v);
                            self.ws();
                            if self.peek() == Some(close) {
                                self.i += 1;
                            }
                        }
                        Err(Broken(code, msg, pos)) => {
                            diags.push(
                                Diagnostic::error(code, format!("In @string {name:?}, {msg}"))
                                    .at(self.line(pos)),
                            );
                            self.resync();
                        }
                    }
                    continue;
                }
                _ => {}
            }
            self.i += 1;
            self.ws();
            let key_start = self.i;
            while self.i < self.b.len()
                && !matches!(self.b[self.i], b',' | b'\n' | b'\r' | b' ' | b'\t')
                && self.b[self.i] != close
            {
                self.i += 1;
            }
            let key = self.src[key_start..self.i].to_string();
            let mut entry = Entry {
                kind,
                key,
                line: self.line(at),
                fields: Vec::new(),
            };
            loop {
                self.ws();
                match self.peek() {
                    None => {
                        diags.push(
                            Diagnostic::error(
                                "bibtex.unclosed",
                                format!(
                                    "Entry {:?} is not closed; kept the fields before the end",
                                    entry.key
                                ),
                            )
                            .at(entry.line),
                        );
                        break;
                    }
                    Some(b',') => {
                        self.i += 1;
                        continue;
                    }
                    Some(c) if c == close => {
                        self.i += 1;
                        break;
                    }
                    Some(b'@') if self.at_line_start(self.i) => {
                        diags.push(
                            Diagnostic::error("bibtex.unclosed", format!("Entry {:?} is not closed before the next entry; kept its fields", entry.key))
                                .at(entry.line),
                        );
                        break;
                    }
                    _ => {}
                }
                let field_at = self.i;
                let name = self.name().to_ascii_lowercase();
                self.ws();
                if name.is_empty() || self.peek() != Some(b'=') {
                    diags.push(
                        Diagnostic::error("bibtex.bad-field", format!("Expected a field name and = in entry {:?}; skipped to the next entry", entry.key))
                            .at(self.line(field_at)),
                    );
                    self.i = field_at + 1;
                    self.resync_field(close);
                    if self.peek() == Some(close) {
                        self.i += 1;
                        break;
                    }
                    continue;
                }
                self.i += 1;
                match self.value(diags) {
                    Ok(v) => entry.fields.push((name, v, self.line(field_at))),
                    Err(Broken(code, msg, pos)) => {
                        diags.push(
                            Diagnostic::error(code, format!("In field {name} of entry {:?}, {msg}; kept the fields before it", entry.key))
                                .at(self.line(pos)),
                        );
                        self.resync();
                        break;
                    }
                }
            }
            out.push(entry);
        }
        if let Some(at) = outside_from {
            if looks_like_lost_fields(self.src[at..].trim()) {
                diags.push(
                    Diagnostic::warning("bibtex.outside-text", "Text outside any entry looks like fields; BibTeX ignores it and so did Pinakes")
                        .at(self.line(at)),
                );
            }
        }
        out
    }

    /// After a malformed field, skip to the next comma at the entry's top level, its close, or
    /// the next entry.
    fn resync_field(&mut self, close: u8) {
        let mut depth = 0usize;
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'{' => depth += 1,
                b'}' if depth > 0 => depth -= 1,
                b',' if depth == 0 => return,
                c if c == close && depth == 0 => return,
                b'@' if self.at_line_start(self.i) => return,
                _ => {}
            }
            self.i += 1;
        }
    }
}

fn looks_like_lost_fields(text: &str) -> bool {
    text.lines().any(|l| {
        let l = l.trim();
        l.split_once('=').is_some_and(|(k, v)| {
            !k.trim().is_empty()
                && k.trim()
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                && v.trim_start().starts_with(['{', '"'])
        })
    })
}

fn month_macro(word: &str) -> Option<&'static str> {
    Some(match word.to_ascii_lowercase().as_str() {
        "jan" => "January",
        "feb" => "February",
        "mar" => "March",
        "apr" => "April",
        "may" => "May",
        "jun" => "June",
        "jul" => "July",
        "aug" => "August",
        "sep" => "September",
        "oct" => "October",
        "nov" => "November",
        "dec" => "December",
        _ => return None,
    })
}

/// BibTeX or BibLaTeX entry type to CSL type.
pub fn kind_from_bib(t: &str) -> &'static str {
    match t {
        "article" => "article-journal",
        "book" | "mvbook" | "collection" | "mvcollection" | "proceedings" | "mvproceedings"
        | "reference" | "mvreference" => "book",
        "booklet" => "pamphlet",
        "inbook" | "incollection" | "bookinbook" | "suppbook" | "suppcollection" => "chapter",
        "inproceedings" | "conference" => "paper-conference",
        "inreference" => "entry-encyclopedia",
        "manual" | "techreport" | "report" => "report",
        "mastersthesis" | "phdthesis" | "thesis" => "thesis",
        "online" | "electronic" | "www" => "webpage",
        "unpublished" => "manuscript",
        "periodical" | "suppperiodical" => "periodical",
        "dataset" => "dataset",
        "software" => "software",
        "patent" => "patent",
        "legislation" | "legal" => "legislation",
        "jurisdiction" => "legal_case",
        "standard" => "standard",
        "letter" => "personal_communication",
        "movie" | "video" => "motion_picture",
        "audio" | "music" => "song",
        "artwork" | "image" => "graphic",
        "performance" => "performance",
        "review" => "review",
        _ => "document",
    }
}

/// CSL type to entry type. BibLaTeX has more types than BibTeX.
pub fn kind_to_bib(kind: &str, biblatex: bool) -> &'static str {
    match (kind, biblatex) {
        (
            "article-journal" | "article-magazine" | "article-newspaper" | "review" | "review-book",
            _,
        ) => "article",
        ("book", _) => "book",
        ("pamphlet", _) => "booklet",
        ("chapter", false) => "incollection",
        ("chapter", true) => "incollection",
        ("paper-conference", _) => "inproceedings",
        ("entry-encyclopedia" | "entry-dictionary" | "entry", true) => "inreference",
        ("report", _) => {
            if biblatex {
                "report"
            } else {
                "techreport"
            }
        }
        ("thesis", true) => "thesis",
        ("thesis", false) => "phdthesis",
        ("webpage" | "post-weblog" | "post", true) => "online",
        ("manuscript", _) => "unpublished",
        ("dataset", true) => "dataset",
        ("software", true) => "software",
        ("patent", true) => "patent",
        ("legislation" | "bill", true) => "legislation",
        ("legal_case", true) => "jurisdiction",
        ("standard", true) => "standard",
        ("periodical", true) => "periodical",
        ("personal_communication", true) => "letter",
        ("motion_picture", true) => "video",
        ("song", true) => "audio",
        ("graphic", true) => "artwork",
        ("performance", true) => "performance",
        _ => "misc",
    }
}

/// Split a name list on `and` at brace depth 0.
fn split_and(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    let words: Vec<&str> = s.split_whitespace().collect();
    for w in words {
        if depth == 0 && w.eq_ignore_ascii_case("and") {
            out.push(std::mem::take(&mut cur));
            continue;
        }
        for c in w.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
    }
    out.push(cur);
    out.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Split on commas at brace depth 0.
fn split_commas(s: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(String::new());
                continue;
            }
            _ => {}
        }
        out.last_mut().unwrap().push(c);
    }
    out.into_iter().map(|p| p.trim().to_string()).collect()
}

/// Words at brace depth 0; a braced group stays in its word.
fn split_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// A word is "von" when its first letter at brace depth 0 is lower case. "{\'e}mile" is not von;
/// "\"uber" counts as lower case.
fn is_von(word: &str) -> bool {
    let mut depth = 0;
    let mut chars = word.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                depth += 1;
                // {\'e} at the start: a special character; BibTeX looks inside.
                if depth == 1 && chars.peek() == Some(&'\\') {
                    let inner: String = chars.clone().take_while(|&c| c != '}').collect();
                    let text = latex_to_text(&inner);
                    return text
                        .chars()
                        .find(|c| c.is_alphabetic())
                        .is_some_and(|c| c.is_lowercase());
                }
            }
            '}' => depth -= 1,
            '\\' if depth == 0 => {
                let rest: String = chars.clone().collect();
                let text = latex_to_text(&format!("\\{rest}"));
                return text
                    .chars()
                    .find(|c| c.is_alphabetic())
                    .is_some_and(|c| c.is_lowercase());
            }
            c if c.is_alphabetic() && depth == 0 => return c.is_lowercase(),
            _ => {}
        }
    }
    false
}

fn txt(s: &str) -> Option<String> {
    let t = latex_to_text(s);
    (!t.is_empty()).then_some(t)
}

/// One BibTeX name: "First von Last", "von Last, First" or "von Last, Jr, First". A name in
/// one pair of braces is an organisation.
pub fn bib_name(raw: &str) -> Name {
    let raw = raw.trim();
    if raw.starts_with('{') && raw.ends_with('}') && balanced_whole(raw) {
        return Name::literal(&latex_to_text(&raw[1..raw.len() - 1]));
    }
    let parts = split_commas(raw);
    let (vonlast, first, jr) = match parts.len() {
        1 => {
            let words = split_words(&parts[0]);
            if words.len() > 1
                && looks_corporate(&latex_to_text(raw))
                && !words.iter().any(|w| is_von(w))
            {
                return Name::literal(&latex_to_text(raw));
            }
            // First von Last: von starts at the first lower-case word that is not the last word.
            let n = words.len();
            let von_start = words[..n.saturating_sub(1)].iter().position(|w| is_von(w));
            match von_start {
                Some(v) => {
                    let mut von_end = v;
                    for (j, w) in words.iter().enumerate().take(n - 1).skip(v) {
                        if is_von(w) {
                            von_end = j + 1;
                        }
                    }
                    let first = words[..v].join(" ");
                    let von = words[v..von_end].join(" ");
                    let last = words[von_end..].join(" ");
                    return make(&first, &von, &last, "");
                }
                None => {
                    let w: Vec<&str> = words.iter().map(String::as_str).collect();
                    let converted: Vec<String> = w.iter().map(|x| latex_to_text(x)).collect();
                    let refs: Vec<&str> = converted.iter().map(String::as_str).collect();
                    let mut n = given_family(&refs);
                    // given_family only splits particles it knows; BibTeX's rule found none.
                    if n.particle.is_some() {
                        n = Name {
                            given: Some(refs[..refs.len() - 1].join(" ")).filter(|g| !g.is_empty()),
                            family: Some(refs[refs.len() - 1].to_string()),
                            ..Name::default()
                        };
                    }
                    return n;
                }
            }
        }
        2 => (parts[0].clone(), parts[1].clone(), String::new()),
        _ => (
            parts[0].clone(),
            parts[parts.len() - 1].clone(),
            parts[1].clone(),
        ),
    };
    let words = split_words(&vonlast);
    let n = words.len();
    let mut von_end = 0;
    for (j, w) in words.iter().enumerate().take(n.saturating_sub(1)) {
        if is_von(w) {
            von_end = j + 1;
        } else {
            break;
        }
    }
    make(
        &first,
        &words[..von_end].join(" "),
        &words[von_end..].join(" "),
        &jr,
    )
}

fn balanced_whole(s: &str) -> bool {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 && i != s.len() - 1 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

fn make(first: &str, von: &str, last: &str, jr: &str) -> Name {
    if last.trim().is_empty() && first.trim().is_empty() {
        return Name::default();
    }
    Name {
        given: txt(first),
        particle: txt(von),
        family: txt(last).or_else(|| txt(von)),
        suffix: txt(jr),
        ..Name::default()
    }
}

fn names(v: &str) -> Vec<Name> {
    split_and(v)
        .iter()
        .filter(|n| !n.eq_ignore_ascii_case("others"))
        .map(|n| bib_name(n))
        .filter(|n| !n.is_empty())
        .collect()
}

/// BibLaTeX dates are ISO 8601 with EDTF marks: `2020-05~` (circa), `2020?`, `2020/2021`,
/// `2020/..`.
fn bib_date(v: &str) -> Option<Date> {
    let t = v.trim();
    let circa = t.contains(['~', '\u{a0}', '%']);
    let uncertain = t.contains('?');
    let cleaned: String = t
        .chars()
        .filter(|c| !matches!(c, '~' | '\u{a0}' | '?' | '%'))
        .collect();
    let cleaned = cleaned.trim_end_matches("/..").trim_start_matches("../");
    let mut d = parse_date(cleaned)?;
    d.circa |= circa;
    if uncertain && d.raw.is_none() {
        d.raw = Some(t.to_string());
    }
    Some(d)
}

fn to_reference(e: Entry, index: usize, diags: &mut Vec<Diagnostic>) -> Reference {
    let mut r = Reference {
        kind: kind_from_bib(&e.kind).to_string(),
        ..Reference::default()
    };
    if kind_to_bib(&r.kind, true) != e.kind && kind_to_bib(&r.kind, false) != e.kind {
        r.keep(FORMAT, "@type", &e.kind);
    }
    if r.kind == "document" && !matches!(e.kind.as_str(), "misc") {
        diags.push(
            Diagnostic::info(
                "bibtex.unknown-type",
                format!(
                    "Entry type @{} has no CSL type; read it as a document",
                    e.kind
                ),
            )
            .at(e.line)
            .record(index),
        );
    }
    if e.key.is_empty() {
        diags.push(
            Diagnostic::warning("bibtex.no-key", "An entry has no citation key")
                .at(e.line)
                .record(index),
        );
    } else {
        r.citation_key = Some(e.key.clone());
    }
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut get: HashMap<String, String> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for (name, value, line) in e.fields {
        if let Some(first) = seen.get(&name) {
            diags.push(
                Diagnostic::warning("bibtex.repeated-field", format!("Field {name} appears again (first on line {first}); kept the second as a source field"))
                    .at(line)
                    .record(index),
            );
            r.keep(FORMAT, &name, &value);
            continue;
        }
        seen.insert(name.clone(), line);
        order.push(name.clone());
        get.insert(name, value);
    }
    let mut used: Vec<&str> = Vec::new();
    let mut take = |k: &'static str| -> Option<String> {
        let v = get.get(k)?;
        used.push(k);
        Some(v.clone())
    };
    let eprinttype = take("eprinttype")
        .or_else(|| take("archiveprefix"))
        .map(|s| s.trim().to_ascii_lowercase());
    let eprint = take("eprint");
    r.title = take("title").and_then(|t| txt(&t));
    if let Some(sub) = take("subtitle").and_then(|s| txt(&s)) {
        r.title = Some(match r.title.take() {
            Some(t) => format!("{t}: {sub}"),
            None => sub,
        });
    }
    r.title_short = take("shorttitle").and_then(|t| txt(&t));
    r.author = take("author").map(|v| names(&v)).unwrap_or_default();
    r.editor = take("editor").map(|v| names(&v)).unwrap_or_default();
    let journal = take("journaltitle").or_else(|| take("journal"));
    let booktitle = take("booktitle");
    match (journal, booktitle) {
        (Some(j), b) => {
            r.container_title = txt(&j);
            if let Some(b) = b {
                r.keep(FORMAT, "booktitle", &b);
            }
        }
        (None, Some(b)) => r.container_title = txt(&b),
        _ => {}
    }
    if r.container_title.is_none() {
        r.container_title = take("maintitle").and_then(|t| txt(&t));
    }
    r.container_title_short = take("shortjournal")
        .or_else(|| take("journalabbr"))
        .or_else(|| take("journal-abbreviation"))
        .and_then(|t| txt(&t));
    r.collection_title = take("series").and_then(|t| txt(&t));
    r.volume = take("volume").and_then(|t| txt(&t));
    let number = take("number").and_then(|t| txt(&t));
    let issue = take("issue").and_then(|t| txt(&t));
    if r.kind == "article-journal" || r.kind == "periodical" {
        r.issue = number.or(issue);
    } else {
        r.number = number;
        r.issue = issue;
    }
    r.page = take("pages").and_then(|t| txt(&t));
    let publisher = take("publisher");
    let institution = take("institution").or_else(|| take("school"));
    let organization = take("organization");
    r.publisher = publisher
        .or(institution)
        .or(organization)
        .and_then(|t| txt(&t));
    r.publisher_place = take("location")
        .or_else(|| take("address"))
        .and_then(|t| txt(&t));
    r.edition = take("edition").and_then(|t| txt(&t));
    r.genre = take("type").and_then(|t| txt(&t));
    if r.genre.is_none() {
        r.genre = match e.kind.as_str() {
            "phdthesis" => Some("PhD thesis".into()),
            "mastersthesis" => Some("Master's thesis".into()),
            _ => None,
        };
    }
    r.abstract_text = take("abstract").and_then(|t| txt(&t));
    r.note = take("note").and_then(|t| txt(&t));
    r.language = take("langid")
        .or_else(|| take("language"))
        .and_then(|t| txt(&t));
    if let Some(k) = take("keywords") {
        r.keywords = latex_to_text(&k)
            .split([',', ';'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
    }
    r.url = take("url")
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty());
    if r.url.is_none() {
        if let Some(h) = get.get("howpublished") {
            if let Some(u) = h.split("\\url{").nth(1).and_then(|s| s.split('}').next()) {
                r.url = Some(u.trim().to_string());
            }
        }
    }
    r.doi = take("doi").map(|d| latex_to_text(&d));
    r.pmid = take("pmid").map(|d| d.trim().to_string());
    r.pmcid = take("pmcid").map(|d| d.trim().to_string());
    r.isbn = take("isbn").map(|d| d.trim().to_string());
    r.issn = take("issn").map(|d| d.trim().to_string());
    if let Some(ep) = eprint {
        match eprinttype.as_deref() {
            Some("arxiv") => r.arxiv = Some(ep.trim().to_string()),
            Some("pubmed") => r.pmid = r.pmid.take().or(Some(ep.trim().to_string())),
            Some("pmcid") | Some("pmc") => r.pmcid = r.pmcid.take().or(Some(ep.trim().to_string())),
            Some("doi") => r.doi = r.doi.take().or(Some(ep.trim().to_string())),
            _ if ep.trim().to_ascii_lowercase().starts_with("arxiv:") => {
                r.arxiv = Some(ep.trim().to_string())
            }
            other => {
                r.keep(FORMAT, "eprint", &ep);
                if let Some(t) = other {
                    r.keep(FORMAT, "eprinttype", t);
                }
            }
        }
    }
    r.issued = match take("date") {
        Some(d) => {
            let parsed = bib_date(&latex_to_text(&d)).map(|mut x| {
                x.circa |= d.contains('~');
                x
            });
            if parsed.is_none() {
                diags.push(
                    Diagnostic::warning("bibtex.bad-date", format!("Could not read date {d:?}"))
                        .at(e.line)
                        .record(index),
                );
            }
            parsed
        }
        None => match take("year") {
            Some(y) => {
                let y = latex_to_text(&y);
                let month = take("month").map(|m| latex_to_text(&m));
                let day = take("day").and_then(|d| d.trim().parse::<u8>().ok());
                match parse_date(&y) {
                    Some(mut d) => {
                        if let (Some(start), Some(m)) = (d.start.as_mut(), month.as_deref()) {
                            if start.month.is_none() {
                                let n = m
                                    .trim()
                                    .parse::<u8>()
                                    .ok()
                                    .filter(|n| (1..=12).contains(n))
                                    .or_else(|| month_number(m.trim()));
                                start.month = n;
                                start.day = n.and(day);
                                if n.is_none() {
                                    r.keep(FORMAT, "month", m);
                                }
                            }
                        }
                        Some(d)
                    }
                    None => Some(Date {
                        literal: Some(y),
                        ..Date::default()
                    }),
                }
            }
            None => None,
        },
    };
    if let Some(u) = take("urldate") {
        r.accessed = bib_date(&u);
    }
    let used: Vec<String> = used.into_iter().map(str::to_string).collect();
    for name in order {
        if used.contains(&name) || (name == "month" && r.issued.is_some()) || name == "day" {
            continue;
        }
        if let Some(v) = get.get(&name) {
            r.keep(FORMAT, &name, v);
        }
    }
    r.id = r
        .citation_key
        .clone()
        .unwrap_or_else(|| (index + 1).to_string());
    finish(&mut r, index, Some(e.line), diags);
    r
}

/// Read BibTeX or BibLaTeX.
pub fn read(text: &str, diags: &mut Vec<Diagnostic>) -> Vec<Reference> {
    let mut lx = Lexer {
        src: text,
        b: text.as_bytes(),
        i: 0,
        lines: LineIndex::new(text),
        strings: HashMap::new(),
    };
    let entries = lx.entries(diags);
    if entries.is_empty() && !text.trim().is_empty() {
        diags.push(Diagnostic::error(
            "bibtex.no-entries",
            "Found no BibTeX entries (an entry starts with @type{key,)",
        ));
    }
    entries
        .into_iter()
        .enumerate()
        .map(|(i, e)| to_reference(e, i, diags))
        .collect()
}

fn write_name(n: &Name, ascii: bool) -> String {
    if let Some(l) = &n.literal {
        return format!("{{{}}}", text_to_latex(l, ascii, false));
    }
    let enc = |s: &str| {
        let t = text_to_latex(s, ascii, false);
        // A family name with spaces ("Lloyd Webber") must not be split.
        t
    };
    let mut family = n.family.as_deref().map(enc).unwrap_or_default();
    if family.contains(' ') || family.contains(',') {
        family = format!("{{{family}}}");
    }
    let mut out = String::new();
    let particle = n
        .particle
        .iter()
        .chain(n.dropping_particle.iter())
        .map(|p| enc(p))
        .collect::<Vec<_>>()
        .join(" ");
    if !particle.is_empty() {
        // Upper-case particles ("Van Gogh" written as family "Gogh", particle "Van") need
        // braces to stay von.
        if particle.chars().next().is_some_and(|c| c.is_uppercase()) {
            out.push_str(&format!("{{{particle}}} "));
        } else {
            out.push_str(&particle);
            out.push(' ');
        }
    }
    out.push_str(&family);
    if let Some(s) = &n.suffix {
        out.push_str(", ");
        out.push_str(&enc(s));
    }
    if let Some(g) = &n.given {
        out.push_str(", ");
        out.push_str(&enc(g));
    } else if n.suffix.is_some() {
        out.push_str(", ");
    }
    out
}

fn bib_page(p: &str) -> String {
    p.replace('-', "--").replace("----", "--")
}

/// Write BibTeX (`biblatex == false`: classic field names, LaTeX accents for anything outside
/// ASCII) or BibLaTeX (`journaltitle`, `date`, `location`, Unicode kept).
pub fn write(refs: &[Reference], biblatex: bool) -> String {
    let keys = crate::key::keys_for_writing(refs);
    let ascii = !biblatex;
    let mut out = String::new();
    for (r, key) in refs.iter().zip(keys) {
        let ty = r
            .source_fields_of(FORMAT)
            .find(|f| f.name == "@type")
            .map(|f| f.value.clone())
            .unwrap_or_else(|| kind_to_bib(&r.kind, biblatex).to_string());
        let mut fields: Vec<(String, String)> = Vec::new();
        let mut add = |name: &str, value: String| {
            if !value.trim().is_empty() {
                fields.push((name.to_string(), value));
            }
        };
        let t = |s: &str| text_to_latex(s, ascii, false);
        if let Some(v) = &r.title {
            add("title", text_to_latex(v, ascii, true));
        }
        if let Some(v) = &r.title_short {
            add("shorttitle", text_to_latex(v, ascii, true));
        }
        if !r.author.is_empty() {
            add(
                "author",
                r.author
                    .iter()
                    .map(|n| write_name(n, ascii))
                    .collect::<Vec<_>>()
                    .join(" and "),
            );
        }
        if !r.editor.is_empty() {
            add(
                "editor",
                r.editor
                    .iter()
                    .map(|n| write_name(n, ascii))
                    .collect::<Vec<_>>()
                    .join(" and "),
            );
        }
        if let Some(c) = &r.container_title {
            let name = match (ty.as_str(), biblatex) {
                ("article" | "periodical", true) => "journaltitle",
                ("article", false) => "journal",
                ("online" | "misc" | "webpage", _) => {
                    if biblatex {
                        "organization"
                    } else {
                        "howpublished"
                    }
                }
                _ => "booktitle",
            };
            add(name, t(c));
        }
        if let Some(c) = &r.container_title_short {
            add(
                if biblatex {
                    "shortjournal"
                } else {
                    "journal-abbreviation"
                },
                t(c),
            );
        }
        if let Some(c) = &r.collection_title {
            add("series", t(c));
        }
        if let Some(v) = &r.volume {
            add("volume", t(v));
        }
        let article = matches!(ty.as_str(), "article" | "periodical");
        match (&r.issue, &r.number) {
            (Some(i), _) if article => add("number", t(i)),
            (i, n) => {
                if let Some(n) = n {
                    add("number", t(n));
                }
                if let Some(i) = i {
                    add(if biblatex { "issue" } else { "number" }, t(i));
                }
            }
        }
        if let Some(p) = &r.page {
            add("pages", t(&bib_page(p)));
        }
        if let Some(e) = &r.edition {
            add("edition", t(e));
        }
        if let Some(p) = &r.publisher {
            let name = match ty.as_str() {
                "phdthesis" | "mastersthesis" => "school",
                "techreport" => "institution",
                "thesis" | "report" if biblatex => "institution",
                _ => "publisher",
            };
            add(name, t(p));
        }
        if let Some(p) = &r.publisher_place {
            add(if biblatex { "location" } else { "address" }, t(p));
        }
        if let Some(g) = &r.genre {
            let implied = (ty == "phdthesis" && g == "PhD thesis")
                || (ty == "mastersthesis" && g == "Master's thesis");
            if !implied {
                add("type", t(g));
            }
        }
        if let Some(d) = &r.issued {
            match (biblatex, d.start) {
                (true, Some(_)) => {
                    let mut iso = d.iso().unwrap_or_default();
                    if d.circa {
                        iso.push('~');
                    }
                    add("date", iso);
                }
                (false, Some(s)) => {
                    add("year", s.year.to_string());
                    if let Some(m) = s.month {
                        const M: [&str; 12] = [
                            "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct",
                            "nov", "dec",
                        ];
                        if let Some(name) = M.get(m as usize - 1) {
                            fields.push(("month".into(), format!("\u{0}{name}")));
                        }
                    }
                }
                (_, None) => {
                    if let Some(l) = &d.literal {
                        add(if biblatex { "date" } else { "year" }, t(l));
                    }
                }
            }
        }
        if let Some(a) = r.accessed.as_ref().and_then(|d| d.iso()) {
            fields.push(("urldate".into(), a));
        }
        let mut add = |name: &str, value: String| {
            if !value.trim().is_empty() {
                fields.push((name.to_string(), value));
            }
        };
        if let Some(v) = &r.doi {
            add("doi", v.clone());
        }
        if let Some(v) = &r.url {
            add("url", v.clone());
        }
        if let Some(v) = &r.isbn {
            add("isbn", v.clone());
        }
        if let Some(v) = &r.issn {
            add("issn", v.clone());
        }
        if let Some(v) = &r.pmid {
            add("pmid", v.clone());
        }
        if let Some(v) = &r.pmcid {
            add("pmcid", v.clone());
        }
        let has_eprint = r.source_fields_of(FORMAT).any(|f| f.name == "eprint");
        if let (Some(a), false) = (&r.arxiv, has_eprint) {
            add("eprint", a.clone());
            add(
                if biblatex {
                    "eprinttype"
                } else {
                    "archiveprefix"
                },
                if biblatex {
                    "arxiv".into()
                } else {
                    "arXiv".into()
                },
            );
        }
        if let Some(v) = &r.language {
            add(
                if biblatex { "langid" } else { "language" },
                text_to_latex(v, ascii, false),
            );
        }
        if !r.keywords.is_empty() {
            add(
                "keywords",
                text_to_latex(&r.keywords.join(", "), ascii, false),
            );
        }
        if let Some(v) = &r.abstract_text {
            add("abstract", text_to_latex(v, ascii, false));
        }
        if let Some(v) = &r.note {
            add("note", text_to_latex(v, ascii, false));
        }
        for f in r.source_fields_of(FORMAT) {
            if f.name == "@type" || fields.iter().any(|(n, _)| *n == f.name) {
                continue;
            }
            fields.push((f.name.clone(), f.value.clone()));
        }
        out.push('@');
        out.push_str(&ty);
        out.push('{');
        out.push_str(&key);
        out.push_str(",\n");
        let width = fields.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
        for (name, value) in &fields {
            out.push_str("  ");
            out.push_str(name);
            for _ in name.len()..width {
                out.push(' ');
            }
            out.push_str(" = ");
            if let Some(bare) = value.strip_prefix('\u{0}') {
                out.push_str(bare);
            } else {
                out.push('{');
                out.push_str(&balance(value));
                out.push('}');
            }
            out.push_str(",\n");
        }
        out.push_str("}\n\n");
    }
    out
}

/// A value must have balanced braces to sit inside `{...}`. Escape the unbalanced ones.
fn balance(v: &str) -> String {
    let mut depth = 0i32;
    let mut ok = true;
    let mut prev = '\0';
    for c in v.chars() {
        if prev != '\\' {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth < 0 {
                        ok = false;
                    }
                }
                _ => {}
            }
        }
        prev = c;
    }
    if ok && depth == 0 {
        return v.to_string();
    }
    let mut out = String::new();
    let mut prev = '\0';
    for c in v.chars() {
        if (c == '{' || c == '}') && prev != '\\' {
            out.push('\\');
        }
        out.push(c);
        prev = c;
    }
    out
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
    fn names_in_three_forms() {
        let n = bib_name("Ludwig van Beethoven");
        assert_eq!(
            (
                n.given.as_deref(),
                n.particle.as_deref(),
                n.family.as_deref()
            ),
            (Some("Ludwig"), Some("van"), Some("Beethoven"))
        );
        let n = bib_name("van der Berg, Jan");
        assert_eq!(
            (
                n.given.as_deref(),
                n.particle.as_deref(),
                n.family.as_deref()
            ),
            (Some("Jan"), Some("van der"), Some("Berg"))
        );
        let n = bib_name("King, Jr, Martin Luther");
        assert_eq!(
            (n.given.as_deref(), n.suffix.as_deref(), n.family.as_deref()),
            (Some("Martin Luther"), Some("Jr"), Some("King"))
        );
        assert_eq!(
            bib_name("{Lunar Survey Collaboration}").literal.as_deref(),
            Some("Lunar Survey Collaboration")
        );
        assert_eq!(
            bib_name("M{\\\"u}ller, J{\\\"o}rg").family.as_deref(),
            Some("Müller")
        );
        let n = bib_name("Jean de La Fontaine");
        assert_eq!(
            (n.particle.as_deref(), n.family.as_deref()),
            (Some("de"), Some("La Fontaine"))
        );
        let n = bib_name("{\\'E}mile Zola");
        assert_eq!(
            (n.given.as_deref(), n.family.as_deref()),
            (Some("Émile"), Some("Zola"))
        );
    }

    #[test]
    fn strings_months_and_concatenation() {
        let (refs, diags) = read_str(
            "@string{ eco = \"Journal of \" }\n@Article{k1,\n  title = {Moths {at} Night},\n  journal = eco # {Ecology},\n  year = 2019, month = mar,\n  author = {Doe, Jane and Roe, R. and others},\n}\n",
        );
        assert_eq!(refs.len(), 1, "{diags:?}");
        let r = &refs[0];
        assert_eq!(r.container_title.as_deref(), Some("Journal of Ecology"));
        assert_eq!(r.issued.as_ref().unwrap().start.unwrap().month, Some(3));
        assert_eq!(r.author.len(), 2);
        assert_eq!(r.citation_key.as_deref(), Some("k1"));
    }

    #[test]
    fn broken_entry_keeps_fields_and_reading_resumes() {
        let (refs, diags) = read_str(
            "@article{a,\n  title = {Kept},\n  journal = {Never closed\n@book{b, title = {Next}}\n",
        );
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].title.as_deref(), Some("Kept"));
        assert_eq!(refs[1].title.as_deref(), Some("Next"));
        assert!(
            diags
                .iter()
                .any(|d| d.code == "bibtex.unclosed" && d.line == Some(3)),
            "{diags:?}"
        );
    }

    #[test]
    fn unknown_fields_round_trip() {
        let src = "@article{k,\n  title = {A},\n  owner = {someone},\n  file = {:a.pdf:PDF},\n}\n";
        let (refs, _) = read_str(src);
        let out = write(&refs, false);
        assert!(out.contains("owner") && out.contains(":a.pdf:PDF"), "{out}");
        let (again, _) = read_str(&out);
        assert_eq!(again, refs);
    }

    #[test]
    fn biblatex_fields() {
        let (refs, _) = read_str("@online{x, title={T}, date={2020-05~}, eprint={2101.00001}, eprinttype={arxiv}, urldate={2021-01-02}, location={Basel}}");
        let r = &refs[0];
        assert!(r.issued.as_ref().unwrap().circa);
        assert_eq!(r.arxiv.as_deref(), Some("2101.00001"));
        assert_eq!(r.publisher_place.as_deref(), Some("Basel"));
        assert_eq!(r.kind, "webpage");
    }

    #[test]
    fn writes_bibtex_and_biblatex() {
        let (refs, _) = read_str("@article{k, title={The DNA of Moths}, author={M{\\\"u}ller, Anna}, journal={Ecology}, year={2020}, month={5}, pages={301--310}}");
        let bib = write(&refs, false);
        assert!(bib.contains("title   = {The {DNA} of Moths}"), "{bib}");
        assert!(bib.contains("M{\\\"u}ller, Anna"), "{bib}");
        assert!(bib.contains("month   = may"), "{bib}");
        assert!(bib.contains("pages   = {301--310}"), "{bib}");
        let blx = write(&refs, true);
        assert!(blx.contains("journaltitle = {Ecology}"), "{blx}");
        assert!(blx.contains("date         = {2020-05}"), "{blx}");
        assert!(blx.contains("Müller, Anna"), "{blx}");
    }
}
