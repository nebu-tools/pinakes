//! Finding records that describe the same publication.
//!
//! Pinakes proposes; the caller decides and merges. The rules are kittum's:
//!
//! 1. The first identifier both records have decides, in the order DOI, PMID, PMCID. The same
//!    value makes a candidate; different values mean two publications, whatever the titles say.
//!    Only identifiers that [`crate::id`] accepts count: a PMID "bad123" is no PMID.
//! 2. Without a shared identifier, two titles that both name a study acronym, phase, part,
//!    follow-up time or study number, and name different ones, are different publications. So
//!    are records more than a year apart, or with different first-author family names.
//! 3. What is left is a candidate when the titles, folded, score at least 90 on token-sort
//!    similarity and the year or first author agrees, or at least 98 on their own.
//!
//! A candidate is a [`Relation::Duplicate`] only when nothing says the two records are different
//! publications. A correction or retraction notice and its article, a comment or reply and its
//! article, a chapter and its book, the same work in two languages, and a preprint and its
//! journal version are candidates with their own relation, so the caller can treat them
//! differently; they never join a group. Groups are joined from the duplicates, never so that
//! one group holds two different DOIs, PMIDs or PMCIDs.
//!
//! # Lossless candidate generation
//!
//! The result is the one comparing every pair gives, and it does not depend on the order of the
//! input. Pinakes does not compare every pair; it generates a set of pairs that provably holds
//! every pair the rules can accept, and compares those:
//!
//! - every pair that shares an identifier (exact-key buckets);
//! - for the rest, only pairs that rule 1 leaves to the titles (no identifier kind on both),
//!   that rule 2 allows (the same first author or one missing, years at most one apart or one
//!   missing), split into buckets by those fields;
//! - within the buckets, only titles whose lengths and character counts can reach the score the
//!   pair needs: a token-sort ratio of at least `t` needs `2·min(la, lb) ≥ t/100·(la + lb)`, and,
//!   since the longest common subsequence holds no more of a character than either title, an
//!   L1 distance between the character-count vectors of at most `(1 − t/100)·(la + lb)`. Both
//!   are necessary conditions of the score, so no candidate is lost. The score needed is 90
//!   where the first author or year can agree, and 98 where neither can.
//!
//! Identical titles land in the same buckets only when the fields that must agree allow it, so
//! many records with one title ("Editorial") cost what their genuine candidates cost.
//! Duplicates are joined into groups in an order fixed by the records' content (identifier
//! matches first, then by score, then by content), so a permutation of the input permutes the
//! result and changes nothing else.

use crate::id::IdKind;
use crate::model::Reference;
use regex_lite::{Captures, Regex};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

/// How two records relate when they are a candidate. Only [`Relation::Duplicate`] says they
/// are the same publication, and only duplicates join groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    /// The same publication.
    Duplicate,
    /// A preprint and its journal version.
    Version,
    /// A correction, retraction or expression of concern and the record it is about.
    Notice,
    /// A comment, letter or reply and the article it answers.
    Comment,
    /// A part and the whole that holds it: a chapter or conference paper and its book.
    Part,
    /// The same work in two languages: a translated edition, or an index's translated title.
    Translation,
}

impl Relation {
    pub fn name(self) -> &'static str {
        match self {
            Relation::Duplicate => "duplicate",
            Relation::Version => "version",
            Relation::Notice => "notice",
            Relation::Comment => "comment",
            Relation::Part => "part",
            Relation::Translation => "translation",
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
    /// Candidate pairs, ordered by `a` then `b`. A duplicate that would have joined two
    /// different identifiers into one group is left out.
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
    prepared: String,
    study_names: HashSet<String>,
    before_numbers: HashSet<String>,
    /// The title's distinguishing tokens, found once.
    tokens: Tokens,
    version: Version,
    role: Role,
    language: Option<String>,
    /// Which identifier kinds the record has: DOI 1, PMID 2, PMCID 4.
    mask: u8,
    /// Character counts of `sorted`, bucketed and capped, for the count filter.
    counts: [u8; COUNT_BUCKETS],
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Version {
    Article,
    Preprint,
    Notice,
    Comment,
}

/// Whether a record is a part of something (a chapter), a whole (a book), or neither.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Role {
    Other,
    Part,
    Whole,
}

fn role(kind: &str) -> Role {
    match kind {
        "chapter" | "entry" | "entry-encyclopedia" | "entry-dictionary" | "paper-conference" => {
            Role::Part
        }
        "book" | "periodical" => Role::Whole,
        _ => Role::Other,
    }
}

/// A language as a two-letter code where it is one of the common ones, so "eng", "en-GB" and
/// "English" agree.
fn language(l: &str) -> Option<String> {
    let l = l.trim().to_lowercase();
    let base = l.split(['-', '_', ' ', ';', ',']).next().unwrap_or("");
    if base.is_empty() {
        return None;
    }
    let code = match base {
        "en" | "eng" | "english" => "en",
        "fr" | "fre" | "fra" | "french" | "français" | "francais" => "fr",
        "de" | "ger" | "deu" | "german" | "deutsch" => "de",
        "es" | "spa" | "spanish" | "español" | "espanol" => "es",
        "it" | "ita" | "italian" | "italiano" => "it",
        "pt" | "por" | "portuguese" | "português" | "portugues" => "pt",
        "nl" | "dut" | "nld" | "dutch" => "nl",
        "ru" | "rus" | "russian" => "ru",
        "zh" | "chi" | "zho" | "chinese" => "zh",
        "ja" | "jpn" | "japanese" => "ja",
        "ko" | "kor" | "korean" => "ko",
        "pl" | "pol" | "polish" => "pl",
        "sv" | "swe" | "swedish" => "sv",
        "no" | "nor" | "nb" | "nob" | "nn" | "nno" | "norwegian" => "no",
        "da" | "dan" | "danish" => "da",
        "fi" | "fin" | "finnish" => "fi",
        "tr" | "tur" | "turkish" => "tr",
        "ar" | "ara" | "arabic" => "ar",
        "he" | "heb" | "hebrew" => "he",
        "el" | "gre" | "ell" | "greek" => "el",
        "cs" | "cze" | "ces" | "czech" => "cs",
        "hu" | "hun" | "hungarian" => "hu",
        other => other,
    };
    Some(code.to_string())
}

const COUNT_BUCKETS: usize = 64;

/// The count-filter bucket of a title character: letters and digits each have their own.
fn bucket(c: char) -> usize {
    match c {
        'a'..='z' => c as usize - 'a' as usize,
        '0'..='9' => 26 + c as usize - '0' as usize,
        ' ' => 36,
        _ => 37 + c as usize % (COUNT_BUCKETS - 37),
    }
}

const NOTICE_TYPES: &[&str] = &[
    "erratum",
    "correction",
    "retraction of publication",
    "expression of concern",
];
/// How the folded title of a notice starts, when its publication type does not say.
const NOTICE_TITLES: &[&str] = &[
    "erratum",
    "errata",
    "corrigendum",
    "corrigenda",
    "correction",
    "retraction",
    "retracted",
    "notice of retraction",
    "expression of concern",
    "withdrawn",
    "withdrawal",
];
/// How the folded title of a comment or reply starts.
const COMMENT_TITLES: &[&str] = &[
    "comment on",
    "comments on",
    "commentary on",
    "reply",
    "in reply",
    "response to",
    "in response",
    "author reply",
    "authors reply",
    "author s reply",
    "authors response",
    "author s response",
    "re ",
    "letter to the editor",
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

fn starts_with_word(normal: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| {
        normal.starts_with(p)
            && (p.ends_with(' ')
                || normal[p.len()..].is_empty()
                || normal[p.len()..].starts_with(' '))
    })
}

fn version(r: &Reference, normal: &str) -> Version {
    let types: Vec<String> = r
        .publication_types
        .iter()
        .map(|t| t.to_lowercase())
        .collect();
    if types
        .iter()
        .any(|t| NOTICE_TYPES.iter().any(|n| t.contains(n)))
        || starts_with_word(normal, NOTICE_TITLES)
    {
        return Version::Notice;
    }
    if types.iter().any(|t| t == "comment" || t == "comment on")
        || starts_with_word(normal, COMMENT_TITLES)
    {
        return Version::Comment;
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
    let t: String = nfc(&crate::text::strip_markup(title))
        .chars()
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
    let mut words: Vec<&str> = normal.split(' ').filter(|w| !w.is_empty()).collect();
    words.sort_unstable();
    let sorted: Vec<char> = words.join(" ").chars().collect();
    let prepared = prepare(title);
    let author_shown = first_author(r);
    // Only identifiers that pass validation: "bad123" is not PMID 123.
    let doi = r.doi.as_deref().and_then(crate::id::doi);
    let pmid = r.pmid.as_deref().and_then(crate::id::pmid);
    let pmcid = r.pmcid.as_deref().and_then(crate::id::pmcid);
    let mask = doi.is_some() as u8 | (pmid.is_some() as u8) << 1 | (pmcid.is_some() as u8) << 2;
    let mut counts = [0u8; COUNT_BUCKETS];
    for c in &sorted {
        let b = &mut counts[bucket(*c)];
        *b = b.saturating_add(1);
    }
    Facts {
        doi,
        pmid,
        pmcid,
        mask,
        counts,
        role: role(&r.kind),
        language: r.language.as_deref().and_then(language),
        year: r.year(),
        author: author_shown.as_ref().map(|a| a.to_lowercase()),
        author_shown,
        sorted,
        study_names: study_names(&prepared),
        before_numbers: words_before_numbers(&prepared),
        tokens: tokens(&prepared),
        prepared,
        version: version(r, &normal),
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
    part: Regex,
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
            // "Part I" and "Part II", "Paper 2", "Pt. 3": parts of one work, each its own
            // publication.
            part: re(r"(?i)\b(?:part|pt|paper)\.?\s*(?P<a>(?:iv|v|i{1,3}|\d+)[a-c]?)\b"),
            number: re(r"^(?P<base>iv|v|i{1,3}|\d+(?:\.\d+)?)(?P<suffix>[a-c]?)$"),
        }
    })
}

const NOT_STUDY_NAMES: &[&str] = &[
    "phase", "week", "weeks", "month", "months", "year", "years", "study", "trial", "part",
    "cohort", "arm", "day", "days", "stage", "grade", "type", "version", "volume", "vol", "no",
    "number",
];

/// NFC, skipping the work for text that is already in it, as almost every title is.
fn nfc(s: &str) -> std::borrow::Cow<'_, str> {
    if s.is_ascii()
        || unicode_normalization::is_nfc_quick(s.chars())
            == unicode_normalization::IsNormalized::Yes
    {
        std::borrow::Cow::Borrowed(s)
    } else {
        std::borrow::Cow::Owned(s.nfc().collect())
    }
}

fn prepare(title: &str) -> String {
    let t: String = nfc(title)
        .chars()
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

/// A title's distinguishing tokens. Which words count as study names depends on the pair, so
/// every word followed by a number is kept with its number; the other kinds are fixed.
#[derive(Default)]
struct Tokens {
    /// Each lower-case word a number follows, with the number.
    numbered: Vec<(String, String)>,
    /// Phases, follow-up times, study numbers and parts.
    kinds: [HashSet<String>; 4],
}

fn tokens(prepared: &str) -> Tokens {
    let p = patterns();
    let mut t = Tokens {
        numbered: letters_before_digits(prepared, true)
            .into_iter()
            .map(|(w, n)| (w.to_lowercase(), number(n)))
            .collect(),
        kinds: Default::default(),
    };
    // Each pattern runs only when the title holds its word; the patterns are slow and the
    // words rare. regex-lite folds only ASCII case, so an ASCII lower-case search is exact.
    let lower = prepared.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    if has(&["phase"]) {
        for m in p.phase.captures_iter(prepared) {
            t.kinds[0].extend(ends(&m));
        }
    }
    if has(&["year", "yr", "month", "week", "day"]) {
        for pat in &p.follow_up {
            for m in pat.captures_iter(prepared) {
                let unit = m["unit"].to_lowercase();
                let unit = if unit == "yr" {
                    "year".to_string()
                } else {
                    unit
                };
                t.kinds[1].extend(ends(&m).into_iter().map(|n| format!("{unit} {n}")));
            }
        }
    }
    if has(&["study", "trial"]) {
        for m in p.study.captures_iter(prepared) {
            t.kinds[2].extend(ends(&m));
        }
    }
    if has(&["part", "pt", "paper"]) {
        for m in p.part.captures_iter(prepared) {
            t.kinds[3].extend(ends(&m));
        }
    }
    t
}

fn tokens_conflict(a: &Facts, b: &Facts) -> bool {
    // The same title has the same tokens, whatever the names.
    if a.prepared == b.prepared {
        return false;
    }
    if (0..4).any(|k| differ(&a.tokens.kinds[k], &b.tokens.kinds[k])) {
        return true;
    }
    // A study name: a word with two capitals in either title, or a word a number follows in
    // both.
    let is_name = |w: &str| {
        a.study_names.contains(w)
            || b.study_names.contains(w)
            || (a.before_numbers.contains(w) && b.before_numbers.contains(w))
    };
    differ(&named(&a.tokens, is_name), &named(&b.tokens, is_name))
}

/// The study names a title numbers, with their numbers.
fn named(t: &Tokens, is_name: impl Fn(&str) -> bool) -> HashSet<(&str, &str)> {
    t.numbered
        .iter()
        .filter(|(w, _)| is_name(w))
        .map(|(w, n)| (w.as_str(), n.as_str()))
        .collect()
}

/// Two token sets conflict when both have tokens and they are not the same.
fn differ<T: Eq + std::hash::Hash>(x: &HashSet<T>, y: &HashSet<T>) -> bool {
    !x.is_empty() && !y.is_empty() && x != y
}

/// Whether two titles name different study acronyms, phases, parts, follow-up times or study
/// numbers.
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

/// How a candidate pair relates. Anything that tells the two records apart wins over
/// `Duplicate`, whatever identifier they share: a chapter can carry its book's DOI, a
/// translation its original's.
fn relation(a: &Facts, b: &Facts) -> Relation {
    let v = [a.version, b.version];
    let differ = a.version != b.version;
    if differ && v.contains(&Version::Notice) {
        Relation::Notice
    } else if differ && v.contains(&Version::Comment) {
        Relation::Comment
    } else if a.role != b.role && a.role != Role::Other && b.role != Role::Other {
        Relation::Part
    } else if matches!((&a.language, &b.language), (Some(x), Some(y)) if x != y) {
        Relation::Translation
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
        if x.abs_diff(y) > 1 {
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
    let score = if a.sorted == b.sorted {
        100.0
    } else {
        ratio_chars(&a.sorted, &b.sorted)
    };
    if score < 90.0 {
        return None;
    }
    let year_match = matches!((a.year, b.year), (Some(x), Some(y)) if x.abs_diff(y) <= 1);
    let author_match = a.author.is_some() && a.author == b.author;
    if !(year_match || author_match || score >= 98.0) {
        return None;
    }
    if tokens_conflict(a, b) {
        return None;
    }
    let mut reasons = vec![Reason::Title { score }];
    if year_match {
        // The earlier year and the first spelling, so the reasons do not depend on which
        // record comes first.
        reasons.push(Reason::Year {
            year: a.year.min(b.year).unwrap_or_default(),
        });
    }
    if author_match {
        let shown = a
            .author_shown
            .clone()
            .min(b.author_shown.clone())
            .unwrap_or_default();
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

/// What candidate generation reads of a record, packed together. Each bucket keeps its own
/// copy, so a scan reads memory in order.
#[derive(Clone, Copy)]
struct Hot {
    counts: [u8; COUNT_BUCKETS],
    len: u32,
    /// `NONE` when missing.
    year: i32,
    /// The first author, interned; `ANON` when missing.
    author: u32,
    /// The record.
    index: u32,
}

const NONE: i32 = i32::MIN;
const ANON: u32 = u32::MAX;

/// Whether the fields rule 2 compares allow the pair: the same first author or one missing,
/// years at most one apart or one missing.
fn may_match(a: &Hot, b: &Hot) -> bool {
    (a.author == ANON || b.author == ANON || a.author == b.author)
        && (a.year == NONE || b.year == NONE || a.year.abs_diff(b.year) <= 1)
}

/// Whether two titles' character counts allow a score of `t`: the longest common subsequence
/// holds no more of a character than either title does. A necessary condition of the score.
fn may_score(a: &Hot, b: &Hot, t: u32) -> bool {
    let mut l1 = 0u32;
    for k in 0..COUNT_BUCKETS {
        l1 += a.counts[k].abs_diff(b.counts[k]) as u32;
    }
    100 * l1 <= (100 - t) * (a.len + b.len)
}

/// Titled records, split by which identifier kinds they have and in title-length order.
#[derive(Default)]
struct Bucket {
    by_mask: [Vec<Hot>; 8],
}

impl Bucket {
    fn push(&mut self, mask: u8, h: Hot) {
        self.by_mask[mask as usize].push(h);
    }
}

/// Every pair `x` in `xs`, `y` in `ys` that rule 1 leaves to the titles (no identifier kind on
/// both), that rule 2 allows, and whose titles can score `t`: the lengths must allow it
/// (`200·min(la, lb) ≥ t·(la + lb)`) and so must the character counts. A bucket joined with
/// itself visits each pair once.
fn join(xs: &Bucket, ys: &Bucket, t: u32, out: &mut Vec<(u32, u32)>) {
    let itself = std::ptr::eq(xs, ys);
    for (mx, xs) in xs.by_mask.iter().enumerate() {
        for (my, ys) in ys.by_mask.iter().enumerate() {
            if mx & my != 0 || (itself && mx > my) || xs.is_empty() || ys.is_empty() {
                continue;
            }
            let same_list = itself && mx == my;
            for (p, hx) in xs.iter().enumerate() {
                let lo = (t * hx.len).div_ceil(200 - t);
                let hi = (200 - t) * hx.len / t;
                let mut from = ys.partition_point(|h| h.len < lo);
                if same_list {
                    from = from.max(p + 1);
                }
                for hy in &ys[from..] {
                    if hy.len > hi {
                        break;
                    }
                    if hx.index != hy.index && may_match(hx, hy) && may_score(hx, hy, t) {
                        let (x, y) = (hx.index, hy.index);
                        out.push((x.min(y), x.max(y)));
                    }
                }
            }
        }
    }
}

/// Run the joins, on every core where there are threads. The result is sorted, so it does not
/// depend on how the work was shared.
fn run_joins(jobs: &[(&Bucket, &Bucket, u32)]) -> Vec<(u32, u32)> {
    let sequential = |jobs: &[(&Bucket, &Bucket, u32)]| {
        let mut out = Vec::new();
        for &(xs, ys, t) in jobs {
            join(xs, ys, t, &mut out);
        }
        out
    };
    let threads = threads();
    if threads < 2 || jobs.len() < 2 {
        return sequential(jobs);
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(&(xs, ys, t)) = jobs.get(k) else {
                            break out;
                        };
                        join(xs, ys, t, &mut out);
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("a join worker panicked"))
            .collect()
    })
}

/// How many threads to use: one per core, or one where there are no threads (WebAssembly).
fn threads() -> usize {
    if cfg!(target_family = "wasm") {
        1
    } else {
        std::thread::available_parallelism().map_or(1, usize::from)
    }
}

/// The pairs to compare: a superset of every pair [`compare`] accepts. See the module
/// documentation for why nothing is lost.
fn pairs_to_compare(facts: &[Facts]) -> Vec<(usize, usize)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    // Rule 1: records that share an identifier, every pair.
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
                out.push((i as u32, j as u32));
            }
        }
    }
    // Rules 2 and 3, on titled records in title-length order.
    let mut authors: HashMap<&str, u32> = HashMap::new();
    let mut titled: Vec<usize> = (0..facts.len())
        .filter(|&i| !facts[i].sorted.is_empty())
        .collect();
    titled.sort_by_key(|&i| (facts[i].sorted.len(), i));
    let mut all = Bucket::default();
    let mut by_author: HashMap<u32, HashMap<i32, Bucket>> = HashMap::new();
    let mut by_author_all: HashMap<u32, Bucket> = HashMap::new();
    let mut by_year: HashMap<i32, Bucket> = HashMap::new();
    let mut anonymous: HashMap<i32, Bucket> = HashMap::new();
    for &i in &titled {
        let f = &facts[i];
        let h = Hot {
            counts: f.counts,
            len: f.sorted.len() as u32,
            year: f.year.unwrap_or(NONE),
            author: f.author.as_deref().map_or(ANON, |a| {
                let next = authors.len() as u32;
                *authors.entry(a).or_insert(next)
            }),
            index: i as u32,
        };
        let m = f.mask;
        if h.author == ANON {
            anonymous.entry(h.year).or_default().push(m, h);
            // Only anonymous records are joined with `all` and `by_year`, and each of those
            // joins covers every pair of two anonymous records too.
        } else {
            by_author
                .entry(h.author)
                .or_default()
                .entry(h.year)
                .or_default()
                .push(m, h);
            by_author_all.entry(h.author).or_default().push(m, h);
        }
        all.push(m, h);
        by_year.entry(h.year).or_default().push(m, h);
    }
    let near = |y: i32, d: i32| {
        if y == NONE {
            None
        } else {
            y.checked_add(d).filter(|z| *z != NONE)
        }
    };
    let mut jobs: Vec<(&Bucket, &Bucket, u32)> = Vec::new();
    // The same first author: the author agrees, so a score of 90 is enough. Years at most one
    // apart, or one missing.
    for (a, years) in &by_author {
        for (&y, xs) in years {
            if y == NONE {
                jobs.push((xs, &by_author_all[a], 90));
                continue;
            }
            for d in [0, 1] {
                if let Some(ys) = near(y, d).and_then(|z| years.get(&z)) {
                    jobs.push((xs, ys, 90));
                }
            }
        }
    }
    // A record without a first author, and any other record: 90 when both years are there and
    // agree, 98 when one is missing.
    for (&y, xs) in &anonymous {
        if y == NONE {
            jobs.push((xs, &all, 98));
            continue;
        }
        for d in [-1, 0, 1] {
            if let Some(ys) = near(y, d).and_then(|z| by_year.get(&z)) {
                jobs.push((xs, ys, 90));
            }
        }
        if let Some(ys) = by_year.get(&NONE) {
            jobs.push((xs, ys, 98));
        }
    }
    // The largest joins first, so no thread is left with one long job at the end.
    let size = |b: &Bucket| b.by_mask.iter().map(Vec::len).sum::<usize>();
    jobs.sort_by_key(|&(xs, ys, _)| std::cmp::Reverse(size(xs) * size(ys)));
    out.extend(run_joins(&jobs));
    out.sort_unstable();
    out.dedup();
    out.into_iter()
        .map(|(a, b)| (a as usize, b as usize))
        .collect()
}

/// Union-find over the records, keeping each group's identifiers so no group gathers two
/// different values of one kind.
struct Groups<'a> {
    facts: &'a [Facts],
    parent: Vec<usize>,
    /// A root's identifiers, once a union has touched it.
    ids: HashMap<usize, [HashSet<&'a str>; 3]>,
}

impl<'a> Groups<'a> {
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
        let mut x = self.ids_of(ra);
        let y = self.ids_of(rb);
        if (0..3).any(|k| x[k].union(&y[k]).count() > 1) {
            self.ids.insert(ra, x);
            self.ids.insert(rb, y);
            return false;
        }
        for (k, set) in y.into_iter().enumerate() {
            x[k].extend(set);
        }
        self.ids.insert(ra, x);
        self.parent[rb] = ra;
        true
    }

    /// Take a root's identifiers out of the map, or read them from its record.
    fn ids_of(&mut self, root: usize) -> [HashSet<&'a str>; 3] {
        self.ids.remove(&root).unwrap_or_else(|| {
            let f = &self.facts[root];
            [&f.doi, &f.pmid, &f.pmcid].map(|v| v.iter().map(String::as_str).collect())
        })
    }
}

/// Each record's place in an order of the records' content, equal for records the rules cannot
/// tell apart. Ties between candidates are broken by it, so the input order does not matter.
fn content_ranks(facts: &[Facts], records: &[usize]) -> HashMap<usize, usize> {
    #[allow(clippy::type_complexity)]
    fn key(
        f: &Facts,
    ) -> (
        &[char],
        &str,
        &Option<String>,
        &Option<String>,
        &Option<String>,
        Option<i32>,
        &Option<String>,
        (Version, Role),
        &Option<String>,
    ) {
        (
            &f.sorted,
            &f.prepared,
            &f.doi,
            &f.pmid,
            &f.pmcid,
            f.year,
            &f.author_shown,
            (f.version, f.role),
            &f.language,
        )
    }
    let mut order = records.to_vec();
    order.sort_by(|&i, &j| key(&facts[i]).cmp(&key(&facts[j])));
    let mut rank = HashMap::with_capacity(order.len());
    for w in 0..order.len() {
        let same = w > 0 && key(&facts[order[w]]) == key(&facts[order[w - 1]]);
        let r = if same { rank[&order[w - 1]] } else { w };
        rank.insert(order[w], r);
    }
    rank
}

/// A score that sorts by `total_cmp`.
#[derive(PartialEq)]
struct OrdScore(f64);

impl Eq for OrdScore {}

impl PartialOrd for OrdScore {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrdScore {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

/// Compare every pair, keeping the candidates in the pairs' order, on every core where there
/// are threads.
fn compare_all(facts: &[Facts], todo: &[(usize, usize)]) -> Vec<Candidate> {
    let run = |pairs: &[(usize, usize)]| -> Vec<Candidate> {
        pairs
            .iter()
            .filter_map(|&(i, j)| compare(i, j, &facts[i], &facts[j]))
            .collect()
    };
    let threads = threads();
    if threads < 2 || todo.len() < 4096 {
        return run(todo);
    }
    let chunk = todo.len().div_ceil(threads);
    std::thread::scope(|scope| {
        let workers: Vec<_> = todo
            .chunks(chunk)
            .map(|c| scope.spawn(move || run(c)))
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("a compare worker panicked"))
            .collect()
    })
}

/// Compare the pairs and join the duplicates into groups.
fn decide(facts: &[Facts], todo: &[(usize, usize)]) -> Duplicates {
    let candidates = compare_all(facts, todo);
    let mut groups = Groups {
        facts,
        parent: (0..facts.len()).collect(),
        ids: HashMap::new(),
    };
    // Only records in a duplicate pair can join a group; the rest stay alone.
    let order: Vec<usize> = (0..candidates.len())
        .filter(|&k| candidates[k].relation == Relation::Duplicate)
        .collect();
    let mut involved: Vec<usize> = order
        .iter()
        .flat_map(|&k| [candidates[k].a, candidates[k].b])
        .collect();
    involved.sort_unstable();
    involved.dedup();
    // Join in an order of content: identifier matches, then the best titles, then by the
    // records' content. Which duplicate a conflict turns away does not depend on input order.
    let rank = content_ranks(facts, &involved);
    let mut keyed: Vec<_> = order
        .into_iter()
        .map(|k| {
            let c = &candidates[k];
            let (x, y) = (rank[&c.a], rank[&c.b]);
            (
                !c.exact(),
                std::cmp::Reverse(OrdScore(c.score)),
                (x.min(y), x.max(y)),
                (c.a, c.b),
                k,
            )
        })
        .collect();
    keyed.sort_unstable();
    let order = keyed.into_iter().map(|key| key.4);
    let mut keep = vec![true; candidates.len()];
    for k in order {
        keep[k] = groups.union(candidates[k].a, candidates[k].b);
    }
    let pairs: Vec<Candidate> = candidates
        .into_iter()
        .zip(keep)
        .filter_map(|(c, k)| k.then_some(c))
        .collect();
    // A group is exact when every duplicate pair kept inside it is.
    let mut inexact: HashSet<usize> = HashSet::new();
    for p in pairs.iter().filter(|p| p.relation == Relation::Duplicate) {
        if !p.exact() {
            inexact.insert(groups.find(p.a));
        }
    }
    let mut members: HashMap<usize, Vec<usize>> = HashMap::new();
    for &i in &involved {
        let root = groups.find(i);
        members.entry(root).or_default().push(i);
    }
    let mut out: Vec<Group> = members
        .into_iter()
        .filter(|(_, m)| m.len() > 1)
        .map(|(root, members)| Group {
            members,
            exact: !inexact.contains(&root),
        })
        .collect();
    out.sort_by_key(|g| g.members[0]);
    Duplicates { pairs, groups: out }
}

/// Every record's facts, in record order, on every core where there are threads.
fn all_facts(refs: &[Reference]) -> Vec<Facts> {
    let threads = threads();
    if threads < 2 || refs.len() < 2048 {
        return refs.iter().map(facts).collect();
    }
    let chunk = refs.len().div_ceil(threads);
    std::thread::scope(|scope| {
        let workers: Vec<_> = refs
            .chunks(chunk)
            .map(|c| scope.spawn(|| c.iter().map(facts).collect::<Vec<_>>()))
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("a facts worker panicked"))
            .collect()
    })
}

/// Find candidate duplicates among references.
pub fn find_duplicates(refs: &[Reference]) -> Duplicates {
    let facts = all_facts(refs);
    let todo = pairs_to_compare(&facts);
    decide(&facts, &todo)
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
    fn candidates_are_what_exhaustive_finds() {
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
        let mut refs: Vec<Reference> = (0..2100)
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
            let src = refs[next(2100)].clone();
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

    /// Every pair, compared: what the generated pairs must reproduce.
    fn exhaustive(refs: &[Reference]) -> Duplicates {
        let facts: Vec<Facts> = refs.iter().map(facts).collect();
        let n = refs.len();
        decide(
            &facts,
            &(0..n)
                .flat_map(|i| (i + 1..n).map(move |j| (i, j)))
                .collect::<Vec<_>>(),
        )
    }

    /// A result in terms of the records' content, so two results compare equal when they say
    /// the same about the same records, whatever their order.
    fn by_content(refs: &[Reference], d: &Duplicates) -> (Vec<String>, Vec<Vec<String>>) {
        let name = |i: usize| format!("{:?}", refs[i]);
        let mut pairs: Vec<String> = d
            .pairs
            .iter()
            .map(|c| {
                let (x, y) = (name(c.a), name(c.b));
                let (x, y) = if x <= y { (x, y) } else { (y, x) };
                format!("{x} {y} {:?} {} {:?}", c.relation, c.score, c.reasons)
            })
            .collect();
        pairs.sort();
        let mut groups: Vec<Vec<String>> = d
            .groups
            .iter()
            .map(|g| {
                let mut m: Vec<String> = g.members.iter().map(|&i| name(i)).collect();
                m.sort();
                m.push(g.exact.to_string());
                m
            })
            .collect();
        groups.sort();
        (pairs, groups)
    }

    fn arbitrary_reference() -> impl proptest::strategy::Strategy<Value = Reference> {
        use proptest::prelude::*;
        let words = prop::sample::select(vec![
            "moth", "moths", "dusk", "forest", "edge", "phase", "ii", "iii", "part", "i",
            "erratum", "reply", "abc", "abc-1", "abc-2", "tide", "Pool", "pool", "12-month",
        ]);
        (
            prop::collection::vec(words, 0..9),
            prop::option::of(1998i32..2002),
            prop::option::of(prop::sample::select(vec!["Adeyemi", "Lind", "Chen"])),
            prop::option::of(prop::sample::select(vec![
                "10.5555/a",
                "10.5555/A",
                "10.5555/b",
                "doi:10.5555/b",
                "10.12/x",
            ])),
            prop::option::of(prop::sample::select(vec!["123", "0123", "bad123", "456"])),
            prop::option::of(prop::sample::select(vec!["PMC1", "PMC2", "PMCx"])),
            prop::sample::select(vec!["article-journal", "chapter", "book"]),
            prop::option::of(prop::sample::select(vec!["eng", "en", "fre"])),
            prop::option::of(prop::sample::select(vec!["Preprint", "Comment", "Erratum"])),
        )
            .prop_map(
                |(words, year, author, doi, pmid, pmcid, kind, lang, ptype)| {
                    let mut r = rec(&words.join(" "), year, author.unwrap_or(""));
                    r.doi = doi.map(str::to_string);
                    r.pmid = pmid.map(str::to_string);
                    r.pmcid = pmcid.map(str::to_string);
                    r.kind = kind.to_string();
                    r.language = lang.map(str::to_string);
                    r.publication_types = ptype.into_iter().map(str::to_string).collect();
                    r
                },
            )
    }

    #[test]
    fn generated_pairs_lose_nothing_and_order_does_not_matter() {
        use proptest::prelude::*;
        let strategy = prop::collection::vec(arbitrary_reference(), 0..40).prop_flat_map(|refs| {
            let n = refs.len();
            (
                Just(refs),
                Just((0..n).collect::<Vec<usize>>()).prop_shuffle(),
            )
        });
        proptest!(|((refs, perm) in strategy)| {
            let fast = find_duplicates(&refs);
            prop_assert_eq!(&fast, &exhaustive(&refs));
            let shuffled: Vec<Reference> = perm.iter().map(|&i| refs[i].clone()).collect();
            prop_assert_eq!(
                by_content(&refs, &fast),
                by_content(&shuffled, &find_duplicates(&shuffled))
            );
        });
    }

    #[test]
    fn identifiers_must_validate() {
        let mut a = rec("Moths at the forest edge", Some(2019), "Adeyemi");
        let mut b = rec("Tides in the estuary", Some(2005), "Lind");
        a.pmid = Some("bad123".into());
        b.pmid = Some("123".into());
        assert!(compare_pair(&a, &b).is_none());
        a.pmid = Some("PMID: 0123".into());
        assert!(compare_pair(&a, &b).unwrap().exact());
        a.pmcid = Some("PMCx".into());
        b.pmcid = Some("PMC9".into());
        assert!(compare_pair(&a, &b).unwrap().exact());
    }

    #[test]
    fn relations_other_than_duplicate() {
        let same_doi = |mut a: Reference, mut b: Reference| {
            a.doi = Some("10.5555/x".into());
            b.doi = Some("10.5555/x".into());
            compare_pair(&a, &b).unwrap().relation
        };
        let moths = || rec("Moths at the forest edge", Some(2019), "Adeyemi");
        // A chapter and its book.
        let mut chapter = moths();
        chapter.kind = "chapter".into();
        let mut book = rec("Night life", Some(2019), "Lind");
        book.kind = "book".into();
        assert_eq!(same_doi(chapter, book), Relation::Part);
        // A correction and its article, by type and by title.
        let mut erratum = moths();
        erratum.publication_types = vec!["Published Erratum".into()];
        assert_eq!(
            compare_pair(&erratum, &moths()).unwrap().relation,
            Relation::Notice
        );
        let erratum = rec("Erratum: Moths at the forest edge", Some(2019), "Adeyemi");
        let original = rec("Moths at the forest edge", Some(2019), "Adeyemi");
        let c = compare_pair(&erratum, &original);
        assert!(c.is_none_or(|c| c.relation == Relation::Notice));
        // A comment and its article.
        let mut comment = moths();
        comment.publication_types = vec!["Comment".into()];
        assert_eq!(
            compare_pair(&comment, &moths()).unwrap().relation,
            Relation::Comment
        );
        let reply = rec(
            "Reply to: The moths at the forest edge at dusk and dawn",
            Some(2019),
            "Adeyemi",
        );
        let article = rec(
            "The moths at the forest edge at dusk and dawn",
            Some(2019),
            "Adeyemi",
        );
        assert_eq!(
            compare_pair(&reply, &article).unwrap().relation,
            Relation::Comment
        );
        // A preprint and the published paper, sharing a PMID.
        let mut pre = moths();
        pre.publication_types = vec!["Preprint".into()];
        pre.pmid = Some("31000001".into());
        let mut published = moths();
        published.pmid = Some("31000001".into());
        let c = compare_pair(&pre, &published).unwrap();
        assert_eq!(c.relation, Relation::Version);
        assert!(!c.exact());
        // Parts I and II.
        assert!(compare_pair(
            &rec(
                "Moths at the forest edge, part I: dusk",
                Some(2019),
                "Adeyemi"
            ),
            &rec(
                "Moths at the forest edge, part II: dusk",
                Some(2019),
                "Adeyemi"
            ),
        )
        .is_none());
        assert!(compare_pair(
            &rec("Moths at the forest edge. Part 1", Some(2019), "Adeyemi"),
            &rec("Moths at the forest edge. Part I", Some(2019), "Adeyemi"),
        )
        .is_some());
        // A translated edition, even with the original's DOI.
        let mut english = moths();
        english.language = Some("eng".into());
        let mut french = moths();
        french.language = Some("fre".into());
        assert_eq!(same_doi(english.clone(), french), Relation::Translation);
        let mut also_english = moths();
        also_english.language = Some("en-GB".into());
        assert_eq!(same_doi(english, also_english), Relation::Duplicate);
    }

    #[test]
    fn only_duplicates_join_groups() {
        let mut a = rec("Moths at the forest edge", Some(2019), "Adeyemi");
        let mut b = a.clone();
        a.kind = "chapter".into();
        b.kind = "book".into();
        a.doi = Some("10.5555/x".into());
        b.doi = Some("10.5555/x".into());
        let d = find_duplicates(&[a, b]);
        assert_eq!(d.pairs.len(), 1);
        assert_eq!(d.pairs[0].relation, Relation::Part);
        assert!(!d.pairs[0].exact());
        assert!(d.groups.is_empty());
    }

    #[test]
    fn many_identical_titles() {
        // 3000 editorials: one title, three authors, 30 years. Only the genuine candidates are
        // compared: the same author and years at most one apart.
        let refs: Vec<Reference> = (0..3000)
            .map(|i| {
                rec(
                    "Editorial",
                    Some(1990 + (i % 30) as i32),
                    ["Adeyemi", "Lind", "Chen"][i / 1000],
                )
            })
            .collect();
        let facts: Vec<Facts> = refs.iter().map(facts).collect();
        let todo = pairs_to_compare(&facts);
        // Per author 1000 records over 30 years: about 33 a year, pairs within and with the
        // next year.
        assert!(
            todo.len() < 3 * 30 * (33 * 33 / 2 + 34 * 34 + 40),
            "{}",
            todo.len()
        );
    }
}
