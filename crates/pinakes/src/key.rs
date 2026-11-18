//! Citation keys: `family` + `year` + the first significant word of the title.
//!
//! `okafor2019moths` for Okafor (2019) "The moths of the forest edge". Keys are ASCII letters
//! and digits: names and titles are folded ("Ødegård" to "odegard"), and scripts with no Latin
//! folding contribute nothing. When two records would share a key, the second gets `a`, the
//! third `b`, and so on, the way Better BibTeX does it. A key a record already has is kept.

use crate::model::Reference;
use crate::text::fold_for_key;
use std::collections::HashSet;

/// Words skipped when picking the title word: articles, prepositions and conjunctions in the
/// languages exports most often carry.
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "of", "on", "in", "at", "to", "for", "from", "by", "with", "and", "or", "but", "as", "is", "are", "was", "were", "be", "into", "onto", "over",
    "under", "about", "via", "vs", "versus", "its", "it", "this", "that", "these", "those", "how", "what", "why", "when", "where", "which", "who", "do", "does", "not",
    "der", "die", "das", "ein", "eine", "und", "zur", "zum", "im", "le", "la", "les", "un", "une", "des", "du", "de", "et", "el", "los", "las", "y", "il", "lo", "gli",
    "di", "da", "het", "een", "en", "van",
];

fn word_part(s: &str) -> String {
    fold_for_key(s).chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// The key a reference would get with no collisions.
pub fn key_for(r: &Reference) -> String {
    let who = r
        .author
        .first()
        .or(r.editor.first())
        .map(|n| match (&n.literal, &n.family) {
            // An organisation: its first significant word.
            (Some(l), _) => l.split_whitespace().map(word_part).find(|w| !w.is_empty() && !STOP_WORDS.contains(&w.as_str())).unwrap_or_default(),
            (None, Some(f)) => {
                let fam = match &n.particle {
                    Some(p) => format!("{p}{f}"),
                    None => f.clone(),
                };
                word_part(&fam)
            }
            (None, None) => n.given.as_deref().map(word_part).unwrap_or_default(),
        })
        .unwrap_or_default();
    let year = r.year().map(|y| y.to_string()).unwrap_or_default();
    let title = r
        .title
        .as_deref()
        .map(crate::text::strip_markup)
        .map(|t| {
            t.split(|c: char| c.is_whitespace() || matches!(c, '-' | '\u{2010}'..='\u{2015}' | '/' | ':' | '.' | ',' | ';'))
                .map(word_part)
                .find(|w| !w.is_empty() && !STOP_WORDS.contains(&w.as_str()))
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let who = if who.is_empty() && title.is_empty() { "anon".to_string() } else { who };
    let key = format!("{who}{year}{title}");
    if key.starts_with(|c: char| c.is_ascii_digit()) {
        format!("k{key}")
    } else {
        key
    }
}

/// `a`, `b`, …, `z`, `aa`, `ab`, …
fn suffix(mut n: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'a' + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

fn unique(base: &str, taken: &mut HashSet<String>) -> String {
    if taken.insert(base.to_string()) {
        return base.to_string();
    }
    let mut i = 0;
    loop {
        let k = format!("{base}{}", suffix(i));
        if taken.insert(k.clone()) {
            return k;
        }
        i += 1;
    }
}

/// Give every reference without a citation key one, unique among all the keys in `refs`.
/// With `replace`, generate keys for every reference and drop the ones they had.
pub fn assign_keys(refs: &mut [Reference], replace: bool) {
    let keys = keys(refs, replace);
    for (r, k) in refs.iter_mut().zip(keys) {
        r.citation_key = Some(k);
    }
}

/// The key each reference is written under: its own, or a generated one.
pub(crate) fn keys_for_writing(refs: &[Reference]) -> Vec<String> {
    keys(refs, false)
}

fn keys(refs: &[Reference], replace: bool) -> Vec<String> {
    let mut taken: HashSet<String> = HashSet::new();
    let mut out: Vec<Option<String>> = vec![None; refs.len()];
    if !replace {
        for (i, r) in refs.iter().enumerate() {
            if let Some(k) = r.citation_key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
                // A repeated key is a collision too; the later record gets a suffix.
                if taken.insert(k.to_string()) {
                    out[i] = Some(k.to_string());
                }
            }
        }
    }
    for (i, r) in refs.iter().enumerate() {
        if out[i].is_none() {
            let base = match r.citation_key.as_deref().map(str::trim).filter(|k| !k.is_empty() && !replace) {
                Some(k) => k.to_string(),
                None => key_for(r),
            };
            out[i] = Some(unique(&base, &mut taken));
        }
    }
    out.into_iter().map(Option::unwrap_or_default).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Date, Name};

    fn r(family: &str, year: i32, title: &str) -> Reference {
        Reference { author: vec![Name::person(family, Some("A."))], issued: Some(Date::year(year)), title: Some(title.into()), ..Reference::default() }
    }

    #[test]
    fn keys() {
        assert_eq!(key_for(&r("Okafor", 2019, "The moths of the forest edge")), "okafor2019moths");
        assert_eq!(key_for(&r("Ødegård", 2001, "À la recherche")), "odegaard2001recherche");
        let mut p = r("Berg", 1999, "Über <i>Lepidoptera</i>");
        p.author[0].particle = Some("van den".into());
        assert_eq!(key_for(&p), "vandenberg1999ueber");
        let org = Reference { author: vec![Name::literal("The Coastal Survey")], title: Some("Report".into()), ..Reference::default() };
        assert_eq!(key_for(&org), "coastalreport");
        assert_eq!(key_for(&Reference::default()), "anon");
    }

    #[test]
    fn collisions_and_overrides() {
        let mut refs = vec![r("Lind", 2020, "Tide pools"), r("Lind", 2020, "Tide pools again"), r("Lind", 2020, "Tide")];
        refs[2].citation_key = Some("mine".into());
        assign_keys(&mut refs, false);
        let k: Vec<_> = refs.iter().map(|r| r.citation_key.clone().unwrap()).collect();
        assert_eq!(k, ["lind2020tide", "lind2020tidea", "mine"]);
        assign_keys(&mut refs, true);
        assert_eq!(refs[2].citation_key.as_deref(), Some("lind2020tideb"));
        assert_eq!(suffix(25), "z");
        assert_eq!(suffix(26), "aa");
    }
}
