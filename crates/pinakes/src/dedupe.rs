//! Finding records that describe the same publication.
//!
//! Pinakes proposes; the caller decides and merges. The rules are kittum's:
//!
//! 1. The first identifier both records have decides, in the order DOI, PMID, PMCID. The same
//!    value makes a candidate; different values mean two publications, whatever the titles say.
//! 2. Without a shared identifier, two titles that both name a study acronym, phase, follow-up time or
//!    study number, and name different ones, are different publications. So are records more
//!    than a year apart, or with different first-author family names.
//! 3. What is left is a candidate when the titles, folded, score at least 90 on token-sort
//!    similarity and the year or first author agrees, or at least 98 on their own.
//!
//! A pair where one record is a correction or retraction notice, or a preprint and the other its
//! journal version, is a candidate with that relation, so the caller can treat it differently.
//! Groups are joined from the candidates, never so that one group holds two different DOIs,
//! PMIDs or PMCIDs.
//!
//! kittum compares every pair. Pinakes does too for small inputs. Above [`EXHAUSTIVE_LIMIT`]
//! records it compares every pair that shares an identifier, and groups the rest into blocks:
//! the whole title, each of the three rarest title words, the two rarest together, and first
//! author with year. Within a block, in title order, each record is compared with the
//! [`WINDOW`] records after it. Similar titles sort near each other, so this finds what
//! comparing every pair finds, at a cost that grows with the input instead of its square.

use crate::id::IdKind;
use crate::model::Reference;
use regex_lite::{Captures, Regex};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

/// Inputs up to this size are compared pair by pair.
pub const EXHAUSTIVE_LIMIT: usize = 2000;

/// Above [`EXHAUSTIVE_LIMIT`], records in a block are put in title order and each is compared
/// with this many that follow it, so a block of up to `WINDOW + 1` records is compared in full.
pub const WINDOW: usize = 16;

/// How two records relate when they are a candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    /// The same publication.
    Duplicate,
    /// A preprint and its journal version.
    Version,
    /// A correction, retraction or expression of concern and the record it is about.
    Notice,
}

impl Relation {
    pub fn name(self) -> &'static str {
        match self {
            Relation::Duplicate => "duplicate",
            Relation::Version => "version",
            Relation::Notice => "notice",
        }
    }
}

/// One piece of evidence for a candidate.
#[derive(Clone, Debug, PartialEq)]
pub enum Reason {
    /// Both records have this identifier.
    Identifier { kind: IdKind, value: String },
    /// The titles' token-sort similarity, 0 to 100.
    Title { score: f64 },
    /// Both records were issued in this year, or within a year of it.
    Year { year: i32 },
    /// Both records' first authors have this family name.
    FirstAuthor { family: String },
}

impl Reason {
    /// The evidence as a sentence fragment, as kittum shows it: "Same DOI 10.5555/x",
    /// "Titles 96% alike", "same year 2024", "same first author Adeyemi".
    pub fn describe(&self) -> String {
        match self {
            Reason::Identifier { kind, value } => format!("Same {} {value}", kind.label()),
            Reason::Title { score } => format!("Titles {score:.0}% alike"),
            Reason::Year { year } => format!("same year {year}"),
            Reason::FirstAuthor { family } => format!("same first author {family}"),
        }
    }
}

/// Two records that may be the same publication. `a < b`, both indices into the input.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub a: usize,
    pub b: usize,
    pub relation: Relation,
    /// 100 for an identifier match, otherwise the title score.
    pub score: f64,
    pub reasons: Vec<Reason>,
}

impl Candidate {
    /// Whether an identifier alone made this candidate a duplicate.
    pub fn exact(&self) -> bool {
        self.relation == Relation::Duplicate
            && matches!(self.reasons.as_slice(), [Reason::Identifier { .. }])
    }
}

/// Records joined through candidates.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// Indices into the input, ascending.
    pub members: Vec<usize>,
    /// Whether every candidate in the group was an exact identifier match.
    pub exact: bool,
}

/// The result of [`find_duplicates`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Duplicates {
    /// Candidate pairs, ordered by `a` then `b`.
    pub pairs: Vec<Candidate>,
    /// Groups of two or more records, ordered by their first member.
    pub groups: Vec<Group>,
}

// ---- Per-record facts, computed once. --------------------------------------------------------

struct Facts {
    doi: Option<String>,
    pmid: Option<String>,
    pmcid: Option<String>,
    year: Option<i32>,
    author: Option<String>,
    author_shown: Option<String>,
    /// The title folded as kittum folds it, tokens sorted, as characters.
    sorted: Vec<char>,
    words: Vec<String>,
    prepared: String,
    study_names: HashSet<String>,
    before_numbers: HashSet<String>,
    version: Version,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Version {
    Article,
    Preprint,
    Notice,
}

const NOTICE_TYPES: &[&str] = &[
    "erratum",
    "correction",
    "retraction of publication",
    "expression of concern",
];
const PREPRINT_DOI_PREFIXES: &[&str] = &[
    "10.1101/",
    "10.48550/",
    "10.21203/",
    "10.2139/",
    "10.20944/",
    "10.31234/",
    "10.31219/",
    "10.22541/",
];

fn version(r: &Reference) -> Version {
    let types: Vec<String> = r
        .publication_types
        .iter()
        .map(|t| t.to_lowercase())
        .collect();
    if types
        .iter()
        .any(|t| NOTICE_TYPES.iter().any(|n| t.contains(n)))
    {
        return Version::Notice;
    }
    let preprint = types.iter().any(|t| t == "preprint")
        || r.id.to_ascii_uppercase().starts_with("PPR:")
        || r.genre
            .as_deref()
            .is_some_and(|g| g.eq_ignore_ascii_case("preprint"))
        || r.doi
            .as_deref()
            .is_some_and(|d| PREPRINT_DOI_PREFIXES.iter().any(|p| d.starts_with(p)));
    if preprint {
        Version::Preprint
    } else {
        Version::Article
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// kittum's `normalize_title`: NFC, lower case, everything but word characters and spaces to a
/// space, spaces collapsed.
pub fn normalise_title(title: &str) -> String {
    let t: String = crate::text::strip_markup(title)
        .nfc()
        .flat_map(char::to_lowercase)
        .map(|c| {
            if is_word(c) || c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect();
    crate::text::squash(&t)
}

/// kittum's first-author rule on a name as one string: the part before a comma; otherwise the
/// first word, unless the last word is short (initials) or the first is.
fn family_from_string(first: &str) -> Option<String> {
    let first = first.trim();
    let family = if let Some((f, _)) = first.split_once(',') {
        f.trim().to_string()
    } else {
        let parts: Vec<&str> = first.split_whitespace().collect();
        match parts.as_slice() {
            [] => return None,
            [one] => one.to_string(),
            [f, .., l] if l.chars().count() <= 2 => f.to_string(),
            [f, .., l] if f.chars().count() <= 2 => l.to_string(),
            [f, ..] => f.to_string(),
        }
    };
    let clean: String = family.chars().filter(|c| is_word(*c)).collect();
    (!clean.is_empty()).then_some(clean)
}

fn first_author(r: &Reference) -> Option<String> {
    let n = r.author.first()?;
    match (&n.literal, &n.family) {
        (Some(l), _) => family_from_string(l),
        (None, Some(f)) => {
            let clean: String = f.chars().filter(|c| is_word(*c)).collect();
            (!clean.is_empty()).then_some(clean)
        }
        (None, None) => None,
    }
}

fn facts(r: &Reference) -> Facts {
    let title = r.title.as_deref().unwrap_or("");
    let normal = normalise_title(title);
    let mut words: Vec<String> = normal
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    words.sort();
    let sorted: Vec<char> = words.join(" ").chars().collect();
    words.dedup();
    let prepared = prepare(title);
    let author_shown = first_author(r);
    Facts {
        doi: r.doi.as_deref().and_then(crate::id::doi),
        pmid: r
            .pmid
            .as_deref()
            .map(|p| p.chars().filter(char::is_ascii_digit).collect::<String>())
            .filter(|p| !p.is_empty()),
        pmcid: r
            .pmcid
            .as_deref()
            .map(|p| p.chars().filter(char::is_ascii_digit).collect::<String>())
            .filter(|p| !p.is_empty())
            .map(|d| format!("PMC{d}")),
        year: r.year(),
        author: author_shown.as_ref().map(|a| a.to_lowercase()),
        author_shown,
        sorted,
        words,
        study_names: study_names(&prepared),
        before_numbers: words_before_numbers(&prepared),
        prepared,
        version: version(r),
    }
}

// ---- Title similarity. -----------------------------------------------------------------------

/// Length of the longest common subsequence, bit-parallel (Hyyrö 2004).
fn lcs(a: &[char], b: &[char]) -> usize {
    let (a, b) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    if a.is_empty() {
        return 0;
    }
    let words = a.len().div_ceil(64);
    let mut masks: HashMap<char, Vec<u64>> = HashMap::new();
    for (i, c) in a.iter().enumerate() {
        masks.entry(*c).or_insert_with(|| vec![0; words])[i / 64] |= 1 << (i % 64);
    }
    let mut s = vec![u64::MAX; words];
    for c in b {
        let Some(m) = masks.get(c) else { continue };
        let mut carry = false;
        for w in 0..words {
            let u = s[w] & m[w];
            let (sum, c1) = s[w].overflowing_add(u);
            let (sum, c2) = sum.overflowing_add(carry as u64);
            carry = c1 || c2;
            s[w] = sum | (s[w] - u);
        }
    }
    let mut zeros = 0;
    for (w, v) in s.iter().enumerate() {
        let bits = if w + 1 == words && a.len() % 64 != 0 {
            a.len() % 64
        } else {
            64
        };
        let mask = if bits == 64 {
            u64::MAX
        } else {
            (1u64 << bits) - 1
        };
        zeros += (!v & mask).count_ones() as usize;
    }
    zeros
}

/// rapidfuzz's `ratio`: normalised Indel similarity, 0 to 100.
#[cfg(test)]
fn ratio(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    ratio_chars(&a, &b)
}

fn ratio_chars(a: &[char], b: &[char]) -> f64 {
    let total = a.len() + b.len();
    if total == 0 {
        return 100.0;
    }
    200.0 * lcs(a, b) as f64 / total as f64
}

/// rapidfuzz's `token_sort_ratio` on kittum-normalised titles.
#[cfg(test)]
fn title_score(a: &str, b: &str) -> f64 {
    let sort = |t: &str| {
        let n = normalise_title(t);
        let mut w: Vec<&str> = n.split(' ').filter(|w| !w.is_empty()).collect();
        w.sort();
        w.join(" ")
    };
    ratio(&sort(a), &sort(b))
}

// ---- Distinguishing tokens. ------------------------------------------------------------------

struct Patterns {
    phase: Regex,
    follow_up: [Regex; 2],
    study: Regex,
    number: Regex,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| {
        // After `prepare`, every dash is "-", so kittum's [-–/] is [-/] here.
        let range = r"(?:\s*[-/]\s*(?P<b>\d+(?:\.\d+)?))?";
        let re = |s: &str| Regex::new(s).expect("pattern compiles");
        Patterns {
            phase: re(r"(?i)\bphase\s*(?P<a>(?:iv|v|i{1,3}|\d+)[a-c]?)(?:\s*[-/]\s*(?P<b>(?:iv|v|i{1,3}|\d+)[a-c]?))?\b"),
            follow_up: [
                re(&format!(r"(?i)\b(?P<a>\d+(?:\.\d+)?){range}[\s-]*(?P<unit>year|yr|month|week|day)s?\b")),
                re(&format!(r"(?i)\b(?P<unit>year|month|week|day)s?\s+(?P<a>\d+){range}\b")),
            ],
            study: re(&format!(r"(?i)\b(?:study|trial)\s*(?:no\.?|number|#)?\s*(?P<a>\d+){range}\b")),
            number: re(r"^(?P<base>iv|v|i{1,3}|\d+(?:\.\d+)?)(?P<suffix>[a-c]?)$"),
        }
    })
}

const NOT_STUDY_NAMES: &[&str] = &[
    "phase", "week", "weeks", "month", "months", "year", "years", "study", "trial", "part",
    "cohort", "arm", "day", "days", "stage", "grade", "type", "version", "volume", "vol", "no",
    "number",
];

fn prepare(title: &str) -> String {
    let t: String = title
        .nfc()
        .map(|c| {
            if matches!(c, '\u{2010}'..='\u{2015}' | '\u{2212}') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let mut out = String::with_capacity(t.len());
    let mut space = false;
    for c in t.chars() {
        if c.is_whitespace() {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

fn number(token: &str) -> String {
    let lower = token.to_lowercase();
    let Some(m) = patterns().number.captures(&lower) else {
        return lower;
    };
    let base = &m["base"];
    let value = match base {
        "i" => "1".to_string(),
        "ii" => "2".to_string(),
        "iii" => "3".to_string(),
        "iv" => "4".to_string(),
        "v" => "5".to_string(),
        b if b.contains('.') => b
            .parse::<f64>()
            .map(format_g)
            .unwrap_or_else(|_| b.to_string()),
        b => b.trim_start_matches('0').to_string(),
    };
    let value = if value.is_empty() {
        "0".to_string()
    } else {
        value
    };
    value + &m["suffix"]
}

/// Python's `format(x, "g")` for the decimals that titles hold.
fn format_g(x: f64) -> String {
    let s = format!("{:.6}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    // Six significant digits, as "g" keeps.
    let digits = s.chars().filter(char::is_ascii_digit).count();
    if digits > 6 {
        format!("{}", (x * 1e6).round() / 1e6)
    } else {
        s.to_string()
    }
}

fn ends(m: &Captures) -> Vec<String> {
    ["a", "b"]
        .iter()
        .filter_map(|g| m.name(g))
        .map(|v| number(v.as_str()))
        .collect()
}

/// Runs of ASCII letters at a word boundary that a digit follows, directly or after one space,
/// underscore or hyphen: the matches of `\b([A-Za-z]+)[\s_-]?(\d+)`, without a regex, since it
/// runs on every title. With `all_digits` a match takes every digit; without, only the first,
/// as `…[\s_-]?\d` does. Returns each run and its digits.
fn letters_before_digits(s: &str, all_digits: bool) -> Vec<(&str, &str)> {
    let b = s.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !(b[i].is_ascii_alphabetic() && (i == 0 || !word(b[i - 1]))) {
            i += 1;
            continue;
        }
        let start = i;
        let mut j = i;
        while j < b.len() && b[j].is_ascii_alphabetic() {
            j += 1;
        }
        let mut k = j;
        if k < b.len()
            && matches!(
                b[k],
                b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | b'_' | b'-'
            )
        {
            k += 1;
        }
        if k < b.len() && b[k].is_ascii_digit() {
            let mut e = k + 1;
            while all_digits && e < b.len() && b[e].is_ascii_digit() {
                e += 1;
            }
            out.push((&s[start..j], &s[k..e]));
            i = e;
        } else {
            // No match starts inside the run: none of its letters is at a word boundary.
            i = j;
        }
    }
    out
}

/// Study names: a run of letters with at least two capitals, followed by a number (`ABC-12`).
fn study_names(prepared: &str) -> HashSet<String> {
    letters_before_digits(prepared, false)
        .into_iter()
        .filter(|(w, _)| w.bytes().filter(u8::is_ascii_uppercase).count() >= 2)
        .map(|(w, _)| w.to_lowercase())
        .collect()
}

fn words_before_numbers(prepared: &str) -> HashSet<String> {
    letters_before_digits(prepared, true)
        .into_iter()
        .map(|(w, _)| w.to_lowercase())
        .filter(|w| w.len() > 1 && !NOT_STUDY_NAMES.contains(&w.as_str()))
        .collect()
}

#[derive(Default)]
struct Tokens {
    kinds: [HashSet<String>; 4],
}

fn tokens(prepared: &str, names: &HashSet<String>) -> Tokens {
    let p = patterns();
    let mut t = Tokens::default();
    for (w, n) in letters_before_digits(prepared, true) {
        let w = w.to_lowercase();
        if names.contains(&w) {
            t.kinds[0].insert(format!("{w} {}", number(n)));
        }
    }
    for m in p.phase.captures_iter(prepared) {
        t.kinds[1].extend(ends(&m));
    }
    for pat in &p.follow_up {
        for m in pat.captures_iter(prepared) {
            let unit = m["unit"].to_lowercase();
            let unit = if unit == "yr" {
                "year".to_string()
            } else {
                unit
            };
            t.kinds[2].extend(ends(&m).into_iter().map(|n| format!("{unit} {n}")));
        }
    }
    for m in p.study.captures_iter(prepared) {
        t.kinds[3].extend(ends(&m));
    }
    t
}

fn tokens_conflict(a: &Facts, b: &Facts) -> bool {
    let mut names: HashSet<String> = a.study_names.union(&b.study_names).cloned().collect();
    names.extend(a.before_numbers.intersection(&b.before_numbers).cloned());
    let ta = tokens(&a.prepared, &names);
    let tb = tokens(&b.prepared, &names);
    (0..4).any(|k| !ta.kinds[k].is_empty() && !tb.kinds[k].is_empty() && ta.kinds[k] != tb.kinds[k])
}

/// Whether two titles name different study acronyms, phases, follow-up times or study numbers.
#[cfg(test)]
fn titles_conflict(a: &str, b: &str) -> bool {
    let fa = facts(&Reference {
        title: Some(a.into()),
        ..Reference::default()
    });
    let fb = facts(&Reference {
        title: Some(b.into()),
        ..Reference::default()
    });
    tokens_conflict(&fa, &fb)
}

// ---- Comparing a pair. -----------------------------------------------------------------------

fn relation(a: &Facts, b: &Facts) -> Relation {
    let v = [a.version, b.version];
    if v.contains(&Version::Notice) && a.version != b.version {
        Relation::Notice
    } else if v.contains(&Version::Preprint) && v.contains(&Version::Article) {
        Relation::Version
    } else {
        Relation::Duplicate
    }
}

fn compare(i: usize, j: usize, a: &Facts, b: &Facts) -> Option<Candidate> {
    for (kind, x, y) in [
        (IdKind::Doi, &a.doi, &b.doi),
        (IdKind::Pmid, &a.pmid, &b.pmid),
        (IdKind::Pmcid, &a.pmcid, &b.pmcid),
    ] {
        if let (Some(x), Some(y)) = (x, y) {
            if x != y {
                return None;
            }
            return Some(Candidate {
                a: i,
                b: j,
                relation: relation(a, b),
                score: 100.0,
                reasons: vec![Reason::Identifier {
                    kind,
                    value: x.clone(),
                }],
            });
        }
    }
    if a.sorted.is_empty() || b.sorted.is_empty() {
        return None;
    }
    if let (Some(x), Some(y)) = (a.year, b.year) {
        if (x - y).abs() > 1 {
            return None;
        }
    }
    if let (Some(x), Some(y)) = (&a.author, &b.author) {
        if x != y {
            return None;
        }
    }
    // ratio >= 90 needs 2 * min / (la + lb) >= 0.9.
    let (la, lb) = (a.sorted.len(), b.sorted.len());
    if 200 * la.min(lb) < 90 * (la + lb) {
        return None;
    }
    let score = ratio_chars(&a.sorted, &b.sorted);
    if score < 90.0 {
        return None;
    }
    let year_match = matches!((a.year, b.year), (Some(x), Some(y)) if (x - y).abs() <= 1);
    let author_match = a.author.is_some() && a.author == b.author;
    if !(year_match || author_match || score >= 98.0) {
        return None;
    }
    if tokens_conflict(a, b) {
        return None;
    }
    let mut reasons = vec![Reason::Title { score }];
    if year_match {
        reasons.push(Reason::Year {
            year: a.year.unwrap_or_default(),
        });
    }
    if author_match {
        let shown = a.author_shown.clone().unwrap_or_default();
        let mut c = shown.chars();
        let family = c
            .next()
            .map(|f| f.to_uppercase().chain(c).collect())
            .unwrap_or_default();
        reasons.push(Reason::FirstAuthor { family });
    }
    Some(Candidate {
        a: i,
        b: j,
        relation: relation(a, b),
        score,
        reasons,
    })
}

/// Compare two references directly.
#[cfg(test)]
fn compare_pair(a: &Reference, b: &Reference) -> Option<Candidate> {
    compare(0, 1, &facts(a), &facts(b))
}

// ---- Candidate generation and grouping. ------------------------------------------------------

fn pairs_to_compare(facts: &[Facts]) -> Vec<(usize, usize)> {
    let n = facts.len();
    if n <= EXHAUSTIVE_LIMIT {
        return (0..n)
            .flat_map(|i| (i + 1..n).map(move |j| (i, j)))
            .collect();
    }
    // Every record's place in title order, so a block sorts by title cheaply.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| facts[i].sorted.cmp(&facts[j].sorted));
    let mut rank = vec![0u32; n];
    for (r, &i) in order.iter().enumerate() {
        rank[i] = r as u32;
    }
    let mut out: Vec<(u32, u32)> = Vec::new();
    let mut pair = |i: usize, j: usize| {
        out.push(if i < j {
            (i as u32, j as u32)
        } else {
            (j as u32, i as u32)
        })
    };
    // Records that share an identifier: every pair.
    let mut by_id: HashMap<(u8, &str), Vec<usize>> = HashMap::new();
    for (i, f) in facts.iter().enumerate() {
        for (k, v) in [(0u8, &f.doi), (1, &f.pmid), (2, &f.pmcid)] {
            if let Some(v) = v {
                by_id.entry((k, v.as_str())).or_default().push(i);
            }
        }
    }
    for members in by_id.values() {
        for (x, &i) in members.iter().enumerate() {
            for &j in &members[x + 1..] {
                pair(i, j);
            }
        }
    }
    // Other blocks: in title order, each record and the next WINDOW.
    let mut blocks: HashMap<Key, Vec<usize>> = HashMap::new();
    let mut df: HashMap<&str, usize> = HashMap::new();
    for f in facts {
        for w in &f.words {
            *df.entry(w.as_str()).or_default() += 1;
        }
    }
    for (i, f) in facts.iter().enumerate() {
        if f.sorted.is_empty() {
            continue;
        }
        // The whole title.
        blocks.entry(Key::Title(&f.sorted)).or_default().push(i);
        // The three rarest title words, and the two rarest together.
        let mut ws: Vec<&str> = f.words.iter().map(String::as_str).collect();
        ws.sort_by_key(|w| (df[w], std::cmp::Reverse(w.len()), *w));
        if let [a, b, ..] = ws[..] {
            blocks
                .entry(Key::Two(a.min(b), a.max(b)))
                .or_default()
                .push(i);
        }
        for w in ws.into_iter().take(3) {
            blocks.entry(Key::Word(w)).or_default().push(i);
        }
        // First author and year.
        if let (Some(a), Some(y)) = (&f.author, f.year) {
            blocks.entry(Key::Author(a, y)).or_default().push(i);
        }
    }
    for members in blocks.values_mut() {
        members.sort_unstable_by_key(|&i| rank[i]);
        for (x, &i) in members.iter().enumerate() {
            for &j in members.iter().skip(x + 1).take(WINDOW) {
                pair(i, j);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out.into_iter()
        .map(|(a, b)| (a as usize, b as usize))
        .collect()
}

#[derive(PartialEq, Eq, Hash)]
enum Key<'a> {
    Title(&'a [char]),
    Word(&'a str),
    Two(&'a str, &'a str),
    Author(&'a str, i32),
}

struct Groups {
    parent: Vec<usize>,
    ids: Vec<[HashSet<String>; 3]>,
}

impl Groups {
    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) -> bool {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return true;
        }
        let conflict = (0..3).any(|k| self.ids[ra][k].union(&self.ids[rb][k]).count() > 1);
        if conflict {
            return false;
        }
        let moved = std::mem::take(&mut self.ids[rb]);
        for (k, set) in moved.into_iter().enumerate() {
            self.ids[ra][k].extend(set);
        }
        self.parent[rb] = ra;
        true
    }
}

/// Find candidate duplicates among references.
pub fn find_duplicates(refs: &[Reference]) -> Duplicates {
    let facts: Vec<Facts> = refs.iter().map(facts).collect();
    let mut groups = Groups {
        parent: (0..refs.len()).collect(),
        ids: facts
            .iter()
            .map(|f| [&f.doi, &f.pmid, &f.pmcid].map(|v| v.iter().cloned().collect()))
            .collect(),
    };
    let mut pairs = Vec::new();
    let todo = pairs_to_compare(&facts);
    for (i, j) in todo {
        if let Some(c) = compare(i, j, &facts[i], &facts[j]) {
            if groups.union(i, j) {
                pairs.push(c);
            }
        }
    }
    let mut members: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..refs.len() {
        let root = groups.find(i);
        members.entry(root).or_default().push(i);
    }
    let mut out: Vec<Group> = members
        .into_values()
        .filter(|m| m.len() > 1)
        .map(|members| {
            let set: HashSet<usize> = members.iter().copied().collect();
            let exact = pairs
                .iter()
                .filter(|p| set.contains(&p.a))
                .all(Candidate::exact);
            Group { members, exact }
        })
        .collect();
    out.sort_by_key(|g| g.members[0]);
    Duplicates { pairs, groups: out }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Date, Name};

    fn rec(title: &str, year: Option<i32>, author: &str) -> Reference {
        Reference {
            title: Some(title.into()),
            issued: year.map(Date::year),
            author: if author.is_empty() {
                vec![]
            } else {
                vec![Name::person(author, Some("A."))]
            },
            ..Reference::default()
        }
    }

    #[test]
    fn indel_ratio_matches_rapidfuzz() {
        assert_eq!(ratio("this is a test", "this is a test!").round(), 97.0);
        assert!((ratio("fuzzy wuzzy was a bear", "wuzzy fuzzy was a bear") - 90.91).abs() < 0.01);
        assert_eq!(
            title_score("fuzzy wuzzy was a bear", "wuzzy fuzzy was a bear"),
            100.0
        );
        let long_a: String = "abcdefghij".repeat(20);
        let long_b: String = format!("x{}", "abcdefghij".repeat(20));
        assert!((ratio(&long_a, &long_b) - 200.0 * 200.0 / 401.0).abs() < 1e-9);
    }

    #[test]
    fn scanners_match_the_patterns_they_replace() {
        let named = Regex::new(r"\b(?P<w>[A-Za-z]+)[\s_-]?(?P<n>\d+)").unwrap();
        let study = Regex::new(r"\b([A-Za-z]*[A-Z][A-Za-z]*[A-Z][A-Za-z]*)[\s_-]?\d").unwrap();
        proptest::proptest!(|(s in "[abAB0-9 _\\-é.]{0,24}")| {
            let want: Vec<(&str, &str)> = named
                .captures_iter(&s)
                .map(|m| (m.name("w").unwrap().as_str(), m.name("n").unwrap().as_str()))
                .collect();
            proptest::prop_assert_eq!(letters_before_digits(&s, true), want);
            let want: HashSet<String> =
                study.captures_iter(&s).map(|c| c[1].to_lowercase()).collect();
            proptest::prop_assert_eq!(study_names(&s), want);
        });
    }

    #[test]
    fn tokens_like_kittum() {
        assert!(titles_conflict(
            "Phase II survey of moth counts",
            "Phase III survey of moth counts"
        ));
        assert!(!titles_conflict(
            "Phase 2 survey of moth counts",
            "Phase II survey of moth counts"
        ));
        assert!(titles_conflict(
            "Moth counts: 12-month follow-up",
            "Moth counts: 24-month follow-up"
        ));
        assert!(titles_conflict(
            "SKYWATCH 1 survey of meteors",
            "SKYWATCH 2 survey of meteors"
        ));
        assert!(!titles_conflict("Meteor survey 2019", "Meteor survey"));
        assert!(titles_conflict("Study 301 of tides", "Study 302 of tides"));
        assert!(titles_conflict(
            "Tidewatch 3 of tides",
            "tidewatch 4 of tides"
        ));
    }

    #[test]
    fn identifiers_decide() {
        let mut a = rec("Moths at the forest edge", Some(2019), "Adeyemi");
        let mut b = rec("Something else entirely", Some(2005), "Lind");
        a.doi = Some("10.5555/x".into());
        b.doi = Some("10.5555/X".into());
        let c = compare_pair(&a, &b).unwrap();
        assert!(c.exact());
        b.doi = Some("10.5555/y".into());
        b.title = a.title.clone();
        assert!(compare_pair(&a, &b).is_none());
        b.doi = Some("10.1101/2019.01.01.000001".into());
        b.pmid = None;
        a.doi = Some("10.1101/2019.01.01.000001".into());
        a.publication_types = vec!["Journal Article".into()];
        let c = compare_pair(&a, &b).unwrap();
        assert_eq!(c.relation, Relation::Duplicate);
        a.doi = None;
        a.pmid = Some("31000001".into());
        b.pmid = Some("31000001".into());
        let c = compare_pair(&a, &b).unwrap();
        assert_eq!(c.relation, Relation::Version);
    }

    #[test]
    fn titles_years_authors() {
        let a = rec("The moths of the forest edge.", Some(2019), "Adeyemi");
        let b = rec("The Moths of the Forest Edge", Some(2020), "Adeyemi");
        let c = compare_pair(&a, &b).unwrap();
        assert_eq!(c.reasons.len(), 3);
        assert_eq!(c.reasons[2].describe(), "same first author Adeyemi");
        assert!(compare_pair(
            &a,
            &rec("The moths of the forest edge", Some(2022), "Adeyemi")
        )
        .is_none());
        assert!(
            compare_pair(&a, &rec("The moths of the forest edge", Some(2019), "Lind")).is_none()
        );
        assert!(compare_pair(&a, &rec("The moths of the forest edge", None, "")).is_some());
        assert!(compare_pair(
            &a,
            &rec("The moths of the forest edges and fields", None, "")
        )
        .is_none());
    }

    #[test]
    fn groups_refuse_conflicting_ids() {
        let mut refs = vec![
            rec("Moths at the forest edge", Some(2019), "Adeyemi"),
            rec("Moths at the forest edge", Some(2019), "Adeyemi"),
            rec("Moths at the forest edge", Some(2019), "Adeyemi"),
        ];
        refs[0].doi = Some("10.5555/a".into());
        refs[2].doi = Some("10.5555/b".into());
        let d = find_duplicates(&refs);
        assert_eq!(d.pairs.len(), 1);
        assert_eq!(d.groups.len(), 1);
        assert_eq!(d.groups[0].members, vec![0, 1]);
        assert!(!d.groups[0].exact);
    }

    #[test]
    fn blocking_finds_what_exhaustive_finds() {
        // Records from a small vocabulary, so titles share many words, and copies changed the
        // ways exports change them: a typo, a word dropped or moved, case and punctuation, the
        // year one off, the author missing.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        let vocab: Vec<String> = (0..300)
            .map(|i| format!("w{}x{}", i * 37 % 300, i % 7))
            .collect();
        let mut refs: Vec<Reference> = (0..EXHAUSTIVE_LIMIT + 100)
            .map(|_| {
                let words: Vec<&str> = (0..8 + next(5))
                    .map(|_| vocab[next(300)].as_str())
                    .collect();
                rec(
                    &words.join(" "),
                    Some(1990 + next(35) as i32),
                    &format!("Family{}", next(60)),
                )
            })
            .collect();
        for k in 0..200 {
            let src = refs[next(EXHAUSTIVE_LIMIT)].clone();
            let mut words: Vec<String> = src
                .title
                .as_deref()
                .unwrap()
                .split(' ')
                .map(String::from)
                .collect();
            match k % 6 {
                0 => {
                    let w = next(words.len());
                    words[w].replace_range(0..1, "q");
                }
                1 => {
                    words.remove(next(words.len()));
                }
                2 => {
                    let (a, b) = (next(words.len()), next(words.len()));
                    words.swap(a, b);
                }
                3 => words[0] = words[0].to_uppercase() + ":",
                _ => {}
            }
            let mut copy = rec(&words.join(" "), src.year(), "");
            if k % 6 == 4 {
                copy.issued = src.year().map(|y| Date::year(y + 1));
            }
            if k % 6 != 5 {
                copy.author = src.author.clone();
            }
            refs.push(copy);
        }
        let facts: Vec<Facts> = refs.iter().map(facts).collect();
        let blocked: HashSet<(usize, usize)> = pairs_to_compare(&facts).into_iter().collect();
        let mut found = 0;
        for i in 0..refs.len() {
            for j in i + 1..refs.len() {
                if compare(i, j, &facts[i], &facts[j]).is_some() {
                    found += 1;
                    assert!(
                        blocked.contains(&(i, j)),
                        "missed {i} {j}: {:?} / {:?}",
                        refs[i].title,
                        refs[j].title
                    );
                }
            }
        }
        assert!(found > 150, "only {found} candidates");
    }
}
