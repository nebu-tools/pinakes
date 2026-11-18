//! Identifiers: DOI, PMID, PMCID, ISBN, ISSN and arXiv, recognised in the forms exports write
//! them and reduced to one normal form each.
//!
//! | Kind  | Accepts                                                        | Normal form            |
//! |-------|----------------------------------------------------------------|------------------------|
//! | DOI   | `10.x/y`, `doi:`, `https://(dx.)doi.org/`, `urn:doi:`, `[doi]`  | `10.x/y`, lower case   |
//! | PMID  | digits, `PMID:` prefix, PubMed URLs                            | digits, no leading 0   |
//! | PMCID | `PMC123`, `PMCID: PMC123`, Europe PMC and NCBI URLs            | `PMC123`               |
//! | ISBN  | ISBN-10 or ISBN-13 with hyphens or spaces, `ISBN` prefix       | ISBN-13 digits         |
//! | ISSN  | `1234-567X`, with or without the hyphen                        | `1234-567X`            |
//! | arXiv | `2101.00001v2`, `hep-th/9901001`, `arXiv:`, arxiv.org URLs, the DataCite DOI | `2101.00001v2` |

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IdKind {
    Doi,
    Pmid,
    Pmcid,
    Isbn,
    Issn,
    Arxiv,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidId {
    pub kind: IdKind,
    pub input: String,
    pub reason: &'static str,
}

impl fmt::Display for InvalidId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} is not a valid {}: {}", self.input, self.kind.label(), self.reason)
    }
}

impl std::error::Error for InvalidId {}

impl IdKind {
    pub const ALL: [IdKind; 6] =
        [IdKind::Doi, IdKind::Pmid, IdKind::Pmcid, IdKind::Isbn, IdKind::Issn, IdKind::Arxiv];

    pub fn label(self) -> &'static str {
        match self {
            IdKind::Doi => "DOI",
            IdKind::Pmid => "PMID",
            IdKind::Pmcid => "PMCID",
            IdKind::Isbn => "ISBN",
            IdKind::Issn => "ISSN",
            IdKind::Arxiv => "arXiv ID",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            IdKind::Doi => "doi",
            IdKind::Pmid => "pmid",
            IdKind::Pmcid => "pmcid",
            IdKind::Isbn => "isbn",
            IdKind::Issn => "issn",
            IdKind::Arxiv => "arxiv",
        }
    }

    pub fn from_name(name: &str) -> Option<IdKind> {
        IdKind::ALL.into_iter().find(|k| k.name().eq_ignore_ascii_case(name))
    }
}

/// Reduce an identifier of a known kind to its normal form, or say why it is not one.
pub fn normalise(kind: IdKind, input: &str) -> Result<String, InvalidId> {
    let fail = |reason| InvalidId { kind, input: input.to_string(), reason };
    let s = input.trim().trim_matches(|c: char| c == '<' || c == '>' || c == '"');
    match kind {
        IdKind::Doi => doi(s).ok_or_else(|| fail("a DOI is 10.<registrant>/<suffix>")),
        IdKind::Pmid => pmid(s).ok_or_else(|| fail("a PMID is 1 to 9 digits")),
        IdKind::Pmcid => pmcid(s).ok_or_else(|| fail("a PMCID is PMC followed by digits")),
        IdKind::Isbn => isbn(s).map_err(fail),
        IdKind::Issn => issn(s).map_err(fail),
        IdKind::Arxiv => arxiv(s).ok_or_else(|| fail("not an arXiv identifier")),
    }
}

/// Recognise any identifier and return its kind and normal form. URLs and prefixed forms are
/// recognised; a bare number is taken as a PMID only when nothing else fits.
pub fn detect(input: &str) -> Option<(IdKind, String)> {
    let s = input.trim();
    let lower = s.to_ascii_lowercase();
    if lower.contains("arxiv") || lower.starts_with("10.48550/") {
        if let Some(a) = arxiv(s) {
            return Some((IdKind::Arxiv, a));
        }
    }
    if let Some(d) = doi(s) {
        return Some((IdKind::Doi, d));
    }
    if let Some(p) = pmcid(s) {
        if lower.contains("pmc") {
            return Some((IdKind::Pmcid, p));
        }
    }
    if lower.contains("isbn") || s.chars().filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x').count() >= 10 {
        if let Ok(i) = isbn(s) {
            return Some((IdKind::Isbn, i));
        }
    }
    if let Ok(i) = issn(s) {
        if s.contains('-') || lower.contains("issn") {
            return Some((IdKind::Issn, i));
        }
    }
    if let Some(a) = arxiv(s) {
        return Some((IdKind::Arxiv, a));
    }
    pmid(s).map(|p| (IdKind::Pmid, p))
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s.is_char_boundary(prefix.len()) && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// DOI normal form: the `10.` name, lower case (DOIs compare without case).
pub fn doi(input: &str) -> Option<String> {
    let mut s = input.trim();
    // PubMed's "10.1/x [doi]".
    if let Some(rest) = s.strip_suffix("[doi]") {
        s = rest.trim();
    }
    for prefix in ["urn:doi:", "info:doi/", "doi:", "doi "] {
        if let Some(rest) = strip_prefix_ci(s, prefix) {
            s = rest.trim();
        }
    }
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.contains("doi.org/") {
        let at = lower.find("doi.org/")?;
        s = &s[at + "doi.org/".len()..];
    }
    let decoded = percent_decode(s);
    let mut d = decoded.trim();
    if !d.starts_with("10.") {
        // A DOI inside running text, as some exports put it ("DOI 10.1/x").
        let at = d.find("10.")?;
        if at > 0 && d.as_bytes()[at - 1].is_ascii_alphanumeric() {
            return None;
        }
        d = &d[at..];
    }
    let d = d.split_whitespace().next()?;
    let d = d.trim_end_matches(['.', ',', ';', ')', ']']);
    let (prefix, suffix) = d.split_once('/')?;
    let registrant = prefix.strip_prefix("10.")?;
    if registrant.is_empty() || !registrant.chars().all(|c| c.is_ascii_digit() || c == '.') || suffix.is_empty() {
        return None;
    }
    let digits = registrant.split('.').next().unwrap_or("");
    if digits.len() < 4 {
        return None;
    }
    Some(d.to_lowercase())
}

/// PMID normal form: digits without leading zeros.
pub fn pmid(input: &str) -> Option<String> {
    let mut s = input.trim();
    for prefix in ["pmid:", "pmid", "pubmed:"] {
        if let Some(rest) = strip_prefix_ci(s, prefix) {
            s = rest.trim();
            break;
        }
    }
    let lower = s.to_ascii_lowercase();
    if lower.contains("pubmed") && lower.contains('/') {
        s = s.trim_end_matches('/').rsplit('/').next()?;
    }
    let s = s.trim_end_matches(['.', ',', ';']);
    let s = s.split_whitespace().next()?;
    if s.is_empty() || s.len() > 10 || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let trimmed = s.trim_start_matches('0');
    if trimmed.is_empty() || trimmed.len() > 9 {
        return None;
    }
    Some(trimmed.to_string())
}

/// PMCID normal form: `PMC` and digits.
pub fn pmcid(input: &str) -> Option<String> {
    let mut s = input.trim();
    for prefix in ["pmcid:", "pmcid"] {
        if let Some(rest) = strip_prefix_ci(s, prefix) {
            s = rest.trim();
            break;
        }
    }
    let lower = s.to_ascii_lowercase();
    if lower.contains('/') {
        s = s.trim_end_matches('/').rsplit('/').next()?;
    }
    let digits = strip_prefix_ci(s, "pmc")?;
    let digits = digits.split_whitespace().next().unwrap_or("").trim_end_matches(['.', ',', ';']);
    if digits.is_empty() || digits.len() > 10 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("PMC{}", digits.trim_start_matches('0')))
}

/// ISBN normal form: the 13 digits, check digit verified. ISBN-10 is converted.
pub fn isbn(input: &str) -> Result<String, &'static str> {
    let mut s = input.trim();
    for prefix in ["isbn-13:", "isbn-10:", "isbn13:", "isbn10:", "isbn:", "isbn"] {
        if let Some(rest) = strip_prefix_ci(s, prefix) {
            s = rest.trim();
            break;
        }
    }
    // "978-3-16-148410-0 (pbk.)": take the leading run of ISBN characters.
    let core: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '-' | ' ' | 'X' | 'x' | '\u{2010}'..='\u{2015}'))
        .filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x')
        .collect();
    match core.len() {
        10 => {
            let mut sum = 0u32;
            for (i, c) in core.chars().enumerate() {
                let v = match c {
                    'X' | 'x' if i == 9 => 10,
                    c if c.is_ascii_digit() => c.to_digit(10).unwrap_or(0),
                    _ => return Err("X may only be the check digit"),
                };
                sum += v * (10 - i as u32);
            }
            if sum % 11 != 0 {
                return Err("the ISBN-10 check digit does not match");
            }
            let body = format!("978{}", &core[..9]);
            Ok(format!("{body}{}", isbn13_check(&body)))
        }
        13 => {
            if !core.chars().all(|c| c.is_ascii_digit()) {
                return Err("an ISBN-13 is digits only");
            }
            if !(core.starts_with("978") || core.starts_with("979")) {
                return Err("an ISBN-13 starts with 978 or 979");
            }
            if isbn13_check(&core[..12]) != core.as_bytes()[12] as char {
                return Err("the ISBN-13 check digit does not match");
            }
            Ok(core)
        }
        _ => Err("an ISBN has 10 or 13 digits"),
    }
}

fn isbn13_check(body12: &str) -> char {
    let sum: u32 = body12
        .chars()
        .enumerate()
        .map(|(i, c)| c.to_digit(10).unwrap_or(0) * if i % 2 == 0 { 1 } else { 3 })
        .sum();
    char::from_digit((10 - sum % 10) % 10, 10).unwrap_or('0')
}

/// ISSN normal form: `1234-567X`, check digit verified.
pub fn issn(input: &str) -> Result<String, &'static str> {
    let mut s = input.trim();
    for prefix in ["eissn:", "issn:", "issn"] {
        if let Some(rest) = strip_prefix_ci(s, prefix) {
            s = rest.trim();
            break;
        }
    }
    let core: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '-' | 'X' | 'x' | '\u{2010}'..='\u{2015}'))
        .filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x')
        .collect::<String>()
        .to_ascii_uppercase();
    if core.len() != 8 || !core[..7].chars().all(|c| c.is_ascii_digit()) {
        return Err("an ISSN has 8 characters");
    }
    let sum: u32 = core[..7].chars().enumerate().map(|(i, c)| c.to_digit(10).unwrap_or(0) * (8 - i as u32)).sum();
    let check = (11 - sum % 11) % 11;
    let expected = if check == 10 { 'X' } else { char::from_digit(check, 10).unwrap_or('0') };
    if core.as_bytes()[7] as char != expected {
        return Err("the ISSN check digit does not match");
    }
    Ok(format!("{}-{}", &core[..4], &core[4..]))
}

/// arXiv normal form: `2101.00001`, `2101.00001v2` or `hep-th/9901001`.
pub fn arxiv(input: &str) -> Option<String> {
    let mut s = input.trim();
    if let Some(rest) = strip_prefix_ci(s, "arxiv:") {
        s = rest.trim();
    }
    let lower = s.to_ascii_lowercase();
    if let Some(at) = lower.find("10.48550/arxiv.") {
        s = &s[at + "10.48550/arxiv.".len()..];
    } else if let Some(at) = lower.find("arxiv.org/") {
        let path = &s[at + "arxiv.org/".len()..];
        let path = path.split_once('/').map(|(_, p)| p).unwrap_or(path);
        s = path.trim_end_matches(".pdf");
    }
    let s = s.split(['?', '#', ' ']).next()?.trim_end_matches(['/', '.', ',', ';']);
    let (body, version) = match s.rfind('v') {
        Some(at) if at + 1 < s.len() && s[at + 1..].chars().all(|c| c.is_ascii_digit()) && at > 0 && s.as_bytes()[at - 1].is_ascii_digit() => {
            (&s[..at], Some(&s[at..]))
        }
        _ => (s, None),
    };
    let new_style = body.split_once('.').is_some_and(|(yymm, num)| {
        yymm.len() == 4
            && yymm.chars().all(|c| c.is_ascii_digit())
            && (4..=5).contains(&num.len())
            && num.chars().all(|c| c.is_ascii_digit())
            && (1..=12).contains(&yymm[2..].parse::<u32>().unwrap_or(0))
    });
    let old_style = body.split_once('/').is_some_and(|(archive, num)| {
        !archive.is_empty()
            && archive.chars().all(|c| c.is_ascii_lowercase() || c == '-' || c == '.' || c.is_ascii_uppercase())
            && archive.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && num.len() == 7
            && num.chars().all(|c| c.is_ascii_digit())
    });
    if !(new_style || old_style) {
        return None;
    }
    let mut out = if old_style {
        let (archive, num) = body.split_once('/')?;
        let (main, sub) = match archive.split_once('.') {
            Some((m, s)) => (m.to_ascii_lowercase(), Some(s.to_ascii_uppercase())),
            None => (archive.to_ascii_lowercase(), None),
        };
        match sub {
            Some(sub) => format!("{main}.{sub}/{num}"),
            None => format!("{main}/{num}"),
        }
    } else {
        body.to_string()
    };
    if let Some(v) = version {
        out.push_str(v);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doi_dresses() {
        for dressed in [
            "10.5555/Moth.2019.001",
            "doi:10.5555/moth.2019.001",
            "DOI: 10.5555/moth.2019.001",
            "https://doi.org/10.5555/moth.2019.001",
            "http://dx.doi.org/10.5555/moth.2019.001",
            "https://www.doi.org/10.5555/moth.2019.001",
            "doi.org/10.5555/moth.2019.001",
            "10.5555/moth.2019.001 [doi]",
            "urn:doi:10.5555/moth.2019.001",
            "https://doi.org/10.5555%2Fmoth.2019.001",
            " 10.5555/moth.2019.001. ",
        ] {
            assert_eq!(doi(dressed).as_deref(), Some("10.5555/moth.2019.001"), "{dressed}");
        }
        assert_eq!(doi("10.1016/S2213-2600(20)30517-8").as_deref(), Some("10.1016/s2213-2600(20)30517-8"));
        assert_eq!(doi("not a doi"), None);
        assert_eq!(doi("10.12/x"), None);
        assert_eq!(doi("10.5555/"), None);
    }

    #[test]
    fn pubmed_ids() {
        assert_eq!(pmid("PMID: 31987691").as_deref(), Some("31987691"));
        assert_eq!(pmid("https://pubmed.ncbi.nlm.nih.gov/31987691/").as_deref(), Some("31987691"));
        assert_eq!(pmid("PMC123"), None);
        assert_eq!(pmid("12a"), None);
        assert_eq!(pmcid("PMC7654321").as_deref(), Some("PMC7654321"));
        assert_eq!(pmcid("pmcid: pmc7654321").as_deref(), Some("PMC7654321"));
        assert_eq!(pmcid("https://www.ncbi.nlm.nih.gov/pmc/articles/PMC7654321/").as_deref(), Some("PMC7654321"));
        assert_eq!(pmcid("7654321"), None);
    }

    #[test]
    fn isbns_and_issns() {
        assert_eq!(isbn("0-306-40615-2").as_deref(), Ok("9780306406157"));
        assert_eq!(isbn("ISBN 978-0-306-40615-7 (pbk.)").as_deref(), Ok("9780306406157"));
        assert!(isbn("978-0-306-40615-8").is_err());
        assert_eq!(isbn("0-8044-2957-X").as_deref(), Ok("9780804429573"));
        assert_eq!(issn("0378-5955").as_deref(), Ok("0378-5955"));
        assert_eq!(issn("2049-3630").as_deref(), Ok("2049-3630"));
        assert!(issn("0378-5954").is_err());
    }

    #[test]
    fn arxiv_ids() {
        assert_eq!(arxiv("arXiv:2101.00001v2").as_deref(), Some("2101.00001v2"));
        assert_eq!(arxiv("https://arxiv.org/abs/2101.00001").as_deref(), Some("2101.00001"));
        assert_eq!(arxiv("https://arxiv.org/pdf/2101.00001v3.pdf").as_deref(), Some("2101.00001v3"));
        assert_eq!(arxiv("10.48550/arXiv.2101.00001").as_deref(), Some("2101.00001"));
        assert_eq!(arxiv("astro-ph/9901001").as_deref(), Some("astro-ph/9901001"));
        assert_eq!(arxiv("math.GT/0309136").as_deref(), Some("math.GT/0309136"));
        assert_eq!(arxiv("2113.00001"), None);
    }

    #[test]
    fn detection() {
        assert_eq!(detect("https://doi.org/10.5555/x"), Some((IdKind::Doi, "10.5555/x".into())));
        assert_eq!(detect("PMC12345"), Some((IdKind::Pmcid, "PMC12345".into())));
        assert_eq!(detect("31987691"), Some((IdKind::Pmid, "31987691".into())));
        assert_eq!(detect("arXiv:2101.00001"), Some((IdKind::Arxiv, "2101.00001".into())));
        assert_eq!(detect("978-0-306-40615-7"), Some((IdKind::Isbn, "9780306406157".into())));
        assert_eq!(detect("0378-5955"), Some((IdKind::Issn, "0378-5955".into())));
    }
}
