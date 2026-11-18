//! Reading personal and corporate names from the strings exports write.

use crate::model::Name;

/// Words that mark a name as an organisation rather than a person.
const CORPORATE: &[&str] = &[
    "academy", "agency", "association", "board", "bureau", "center", "centre", "college",
    "collaboration", "collaborators", "commission", "committee", "consortium", "council",
    "department", "federation", "foundation", "group", "inc", "initiative", "institute",
    "institution", "laboratory", "league", "ltd", "ministry", "museum", "network", "office",
    "organisation", "organization", "programme", "program", "project", "service", "services",
    "society", "survey", "team", "trust", "union", "university", "observatory", "library",
    "archive", "archives", "school", "gmbh", "ag", "llc", "plc", "institut", "universität",
    "universitat", "université", "universite", "universidad", "universidade", "università",
    "gesellschaft", "verein", "instituto", "istituto", "stiftung", "hospital", "clinic",
    "authority", "corporation", "company", "co", "partners", "working",
];

/// Lower-case words that start a family name (non-dropping particles).
const PARTICLES: &[&str] = &[
    "van", "von", "der", "den", "de", "del", "della", "di", "da", "dos", "das", "du", "la", "le",
    "ten", "ter", "zu", "zur", "af", "av", "y", "bin", "ibn", "al", "el", "d'", "l'", "st",
];

const SUFFIXES: &[&str] = &["jr", "jr.", "sr", "sr.", "ii", "iii", "iv", "v", "2nd", "3rd"];

/// Whether a comma-free name string reads as an organisation.
pub fn looks_corporate(s: &str) -> bool {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .any(|w| CORPORATE.contains(&w.to_lowercase().as_str()))
}

/// Whether a token is initials: "J", "JA", "J.A.", "J.-P.", "M.W".
pub fn is_initials(token: &str) -> bool {
    let letters: Vec<char> = token.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() || letters.len() > 4 {
        return false;
    }
    let all_upper = letters.iter().all(|c| c.is_uppercase());
    let only_marks = token.chars().all(|c| c.is_alphabetic() || matches!(c, '.' | '-' | '\u{2010}'));
    if !all_upper || !only_marks {
        return false;
    }
    // "LU" or "JA" are initials; "Li" is a name.
    token.contains('.') || letters.len() <= 3
}

/// "JA" to "J. A.", "J.-P." kept, "M.W." to "M. W.".
pub fn expand_initials(token: &str) -> String {
    let mut out = String::new();
    let mut chars = token.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_alphabetic() {
            if !out.is_empty() && !out.ends_with('-') {
                out.push(' ');
            }
            out.push(c);
            out.push('.');
            while chars.peek() == Some(&'.') {
                chars.next();
            }
        } else if c == '-' || c == '\u{2010}' {
            out.push('-');
        }
    }
    out
}

fn split_particle(family: &str) -> (Option<String>, String) {
    let words: Vec<&str> = family.split_whitespace().collect();
    let mut i = 0;
    while i + 1 < words.len() && is_particle(words[i]) {
        i += 1;
    }
    if i == 0 {
        (None, family.trim().to_string())
    } else {
        (Some(words[..i].join(" ")), words[i..].join(" "))
    }
}

fn is_particle(word: &str) -> bool {
    word.chars().next().is_some_and(|c| c.is_lowercase()) && (PARTICLES.contains(&word) || word.len() <= 4)
}

fn clean_given(given: &str) -> Option<String> {
    let g = given.trim();
    if g.is_empty() {
        return None;
    }
    if !g.contains(' ') && is_initials(g) {
        return Some(expand_initials(g));
    }
    Some(g.to_string())
}

fn person(family: &str, given: Option<&str>, suffix: Option<&str>) -> Name {
    let (particle, family) = split_particle(family);
    Name {
        family: Some(family),
        given: given.and_then(clean_given),
        particle,
        suffix: suffix.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string),
        ..Name::default()
    }
}

/// Read a name as RIS, MEDLINE, Web of Science and spreadsheets write it.
///
/// "Family, Given", "Family, Given, Suffix", "Family Initials" (Vancouver), "Given Family", a
/// single word, or an organisation. A comma-free name of several words that is not in
/// Vancouver form is read as an organisation, as RIS defines persons with a comma.
pub fn parse_name(input: &str) -> Name {
    let s = crate::text::squash(input.trim().trim_end_matches([';', ',']));
    if s.is_empty() {
        return Name::default();
    }
    if looks_corporate(&s) {
        return Name::literal(&s);
    }
    if s.contains(',') {
        let parts: Vec<&str> = s.split(',').map(str::trim).collect();
        if parts.len() > 3 || parts[0].is_empty() || parts[1..].iter().any(|p| p.split_whitespace().count() > 4) {
            return Name::literal(&s);
        }
        let (given, suffix) = match parts.len() {
            1 => (None, None),
            2 if SUFFIXES.contains(&parts[1].to_lowercase().as_str()) => (None, Some(parts[1])),
            2 => (Some(parts[1]), None),
            _ => {
                // RIS: "Family, Given, Suffix"; BibTeX: "von Last, Jr, First".
                if SUFFIXES.contains(&parts[1].to_lowercase().as_str()) {
                    (Some(parts[2]), Some(parts[1]))
                } else {
                    (Some(parts[1]), Some(parts[2]))
                }
            }
        };
        return person(parts[0], given, suffix);
    }
    let words: Vec<&str> = s.split(' ').collect();
    if words.len() == 1 {
        return Name { family: Some(s), ..Name::default() };
    }
    let last = words[words.len() - 1];
    if is_initials(last) && !is_initials(words[0]) {
        // Vancouver: "Smith JA", "van der Berg J", "Konstan M.W.".
        return person(&words[..words.len() - 1].join(" "), Some(last), None);
    }
    if words.iter().all(|w| w.chars().next().is_some_and(|c| c.is_uppercase() || !c.is_alphabetic()) || is_particle(w))
        && words.len() <= 4
    {
        // "Ludwig van Beethoven", "J. R. R. Tolkien".
        return given_family(&words);
    }
    Name::literal(&s)
}

/// "Given [particles] Family" in BibTeX's first form.
pub fn given_family(words: &[&str]) -> Name {
    if words.is_empty() {
        return Name::default();
    }
    if words.len() == 1 {
        return Name { family: Some(words[0].to_string()), ..Name::default() };
    }
    // The particle is the first run of lower-case words after the first word.
    let start = words[1..].iter().position(|w| is_particle(w)).map(|p| p + 1);
    match start {
        Some(p) if p < words.len() - 1 => {
            let mut end = p;
            while end < words.len() - 1 && is_particle(words[end]) {
                end += 1;
            }
            Name {
                given: Some(words[..p].join(" ")),
                particle: Some(words[p..end].join(" ")),
                family: Some(words[end..].join(" ")),
                ..Name::default()
            }
        }
        _ => Name {
            given: clean_given(&words[..words.len() - 1].join(" ")),
            family: Some(words[words.len() - 1].to_string()),
            ..Name::default()
        },
    }
}

/// Split an author list written in one field: "Smith J; Doe AB", "Smith, J.; Doe, A.",
/// "Smith J, Doe AB" (Vancouver), one name per line.
pub fn split_name_list(s: &str) -> Vec<String> {
    let s = s.trim();
    if s.is_empty() {
        return Vec::new();
    }
    if s.contains(';') || s.contains('\n') {
        return s.split([';', '\n']).map(str::trim).filter(|p| !p.is_empty()).map(str::to_string).collect();
    }
    let parts: Vec<&str> = s.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    // Vancouver: every comma-separated part ends in initials.
    if parts.len() > 1
        && parts.iter().all(|p| {
            let words: Vec<&str> = p.split_whitespace().collect();
            words.len() >= 2 && is_initials(words[words.len() - 1])
        })
    {
        return parts.into_iter().map(str::to_string).collect();
    }
    vec![s.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fam(n: &Name) -> (&str, &str) {
        (n.family.as_deref().unwrap_or(""), n.given.as_deref().unwrap_or(""))
    }

    #[test]
    fn forms() {
        assert_eq!(fam(&parse_name("Wainwright, CE")), ("Wainwright", "C. E."));
        assert_eq!(fam(&parse_name("Konstan, M.W.")), ("Konstan", "M. W."));
        assert_eq!(fam(&parse_name("Konstan M.W.")), ("Konstan", "M. W."));
        assert_eq!(fam(&parse_name("Smith JA")), ("Smith", "J. A."));
        assert_eq!(fam(&parse_name("Turner, Nicholas C.")), ("Turner", "Nicholas C."));
        assert_eq!(fam(&parse_name("Gabler")), ("Gabler", ""));
        let n = parse_name("van der Berg, Jan");
        assert_eq!((n.particle.as_deref(), fam(&n)), (Some("van der"), ("Berg", "Jan")));
        let n = parse_name("Ludwig van Beethoven");
        assert_eq!((n.particle.as_deref(), fam(&n)), (Some("van"), ("Beethoven", "Ludwig")));
        let n = parse_name("King, Martin Luther, Jr.");
        assert_eq!((n.suffix.as_deref(), fam(&n)), (Some("Jr."), ("King", "Martin Luther")));
    }

    #[test]
    fn organisations() {
        for s in [
            "World Meteorological Organization",
            "University Hospital, Basel, Switzerland",
            "Lunar Survey Collaboration",
            "Muster Seed Company GmbH",
        ] {
            assert_eq!(parse_name(s).literal.as_deref(), Some(s), "{s}");
        }
    }

    #[test]
    fn lists() {
        assert_eq!(split_name_list("Smith J, Doe AB"), vec!["Smith J", "Doe AB"]);
        assert_eq!(split_name_list("Smith, J.; Doe, A."), vec!["Smith, J.", "Doe, A."]);
        assert_eq!(split_name_list("Smith, Jane"), vec!["Smith, Jane"]);
    }
}
