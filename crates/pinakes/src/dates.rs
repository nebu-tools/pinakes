//! Reading dates in the forms exports write them.

use crate::model::{Date, Ymd};

const MONTHS: &[(&str, u8)] = &[
    ("january", 1),
    ("february", 2),
    ("march", 3),
    ("april", 4),
    ("may", 5),
    ("june", 6),
    ("july", 7),
    ("august", 8),
    ("september", 9),
    ("october", 10),
    ("november", 11),
    ("december", 12),
    ("jan", 1),
    ("feb", 2),
    ("mar", 3),
    ("apr", 4),
    ("jun", 6),
    ("jul", 7),
    ("aug", 8),
    ("sep", 9),
    ("sept", 9),
    ("oct", 10),
    ("nov", 11),
    ("dec", 12),
    // German and French, common in European exports.
    ("januar", 1),
    ("februar", 2),
    ("märz", 3),
    ("mai", 5),
    ("juni", 6),
    ("juli", 7),
    ("oktober", 10),
    ("dezember", 12),
    ("janvier", 1),
    ("février", 2),
    ("mars", 3),
    ("avril", 4),
    ("juin", 6),
    ("juillet", 7),
    ("août", 8),
    ("septembre", 9),
    ("octobre", 10),
    ("novembre", 11),
    ("décembre", 12),
];

const SEASONS: &[&str] = &["spring", "summer", "autumn", "fall", "winter"];

pub fn month_number(word: &str) -> Option<u8> {
    let w = word.trim_end_matches('.').to_lowercase();
    MONTHS.iter().find(|(name, _)| *name == w).map(|(_, n)| *n)
}

#[derive(Debug, PartialEq)]
enum Tok {
    Num(u32, usize),
    Month(u8),
    Season,
    Circa,
    Dash,
    Word,
}

fn tokens(s: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c.is_ascii_digit() {
            let mut end = i + c.len_utf8();
            while let Some(&(j, d)) = chars.peek() {
                if d.is_ascii_digit() {
                    end = j + 1;
                    chars.next();
                } else {
                    break;
                }
            }
            let digits = &s[i..end];
            out.push(Tok::Num(digits.parse().unwrap_or(u32::MAX), digits.len()));
        } else if c.is_alphabetic() {
            let mut end = i + c.len_utf8();
            while let Some(&(j, d)) = chars.peek() {
                if d.is_alphabetic() {
                    end = j + d.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            let word = &s[i..end];
            let lower = word.to_lowercase();
            if let Some(m) = month_number(word) {
                out.push(Tok::Month(m));
            } else if SEASONS.contains(&lower.as_str()) {
                out.push(Tok::Season);
            } else if matches!(lower.as_str(), "c" | "ca" | "circa") {
                out.push(Tok::Circa);
            } else {
                out.push(Tok::Word);
            }
        } else if matches!(c, '-' | '\u{2010}'..='\u{2015}') && !out.is_empty() {
            out.push(Tok::Dash);
        }
    }
    out
}

fn plausible_year(n: u32, digits: usize) -> Option<i32> {
    (digits == 4 && (1000..=2999).contains(&n)).then_some(n as i32)
}

/// Read a date. Returns `None` for empty input. A string with no year becomes a literal date.
///
/// Handles `2020`, `2020/05/01`, `2020/05//`, `2020///`, `2020-05-01`, `2020 May 1`,
/// `1 May 2020`, `May 2020`, `2020 Jan-Feb`, `2019-2020`, `c. 1850`, `Summer 2020`,
/// `05/01/2020` (read as day/month/year only when unambiguous) and ISO ranges `2020/2021`.
pub fn parse_date(input: &str) -> Option<Date> {
    let s = input.trim();
    if s.is_empty() || s.chars().all(|c| matches!(c, '/' | '-' | '.' | ' ')) {
        return None;
    }
    let toks = tokens(s);
    let mut date = Date {
        circa: toks.contains(&Tok::Circa),
        ..Date::default()
    };

    // ISO 8601: 2020, 2020-05, 2020-05-01, 2020-05-01T12:00:00Z, and intervals 2020/2021.
    let iso = match s.find('T') {
        Some(at) if at >= 10 && s[..at].bytes().all(|b| b.is_ascii_digit() || b == b'-') => {
            &s[..at]
        }
        _ => s,
    };
    if let Some(ymd) = parse_simple(iso) {
        date.start = Some(ymd);
        return Some(date);
    }
    if let Some((a, b)) = s.split_once('/') {
        if let (Some(x), Some(y)) = (parse_simple(a), parse_simple(b)) {
            date.start = Some(x);
            date.end = Some(y);
            return Some(date);
        }
    }

    let years: Vec<(usize, i32)> = toks
        .iter()
        .enumerate()
        .filter_map(|(i, t)| match t {
            Tok::Num(n, d) => plausible_year(*n, *d).map(|y| (i, y)),
            _ => None,
        })
        .collect();
    let Some(&(year_at, year)) = years.first() else {
        if toks.iter().any(|t| matches!(t, Tok::Num(..))) {
            // Two-digit or unusual years: keep as a literal.
        }
        date.literal = Some(s.to_string());
        return Some(date);
    };
    let mut start = Ymd {
        year,
        month: None,
        day: None,
    };
    let mut lossy = toks.iter().any(|t| matches!(t, Tok::Season | Tok::Word));

    let nums_after: Vec<u32> = toks[year_at + 1..]
        .iter()
        .take_while(|t| !matches!(t, Tok::Dash))
        .filter_map(|t| match t {
            Tok::Num(n, _) => Some(*n),
            _ => None,
        })
        .collect();
    let month_tok = toks.iter().position(|t| matches!(t, Tok::Month(_)));

    if let Some(mi) = month_tok {
        if let Tok::Month(m) = toks[mi] {
            start.month = Some(m);
        }
        // Day: a 1-2 digit number next to the month.
        let day = toks
            .get(mi + 1)
            .or(if mi > 0 { toks.get(mi - 1) } else { None });
        let day = match (
            toks.get(mi + 1),
            if mi > 0 { toks.get(mi - 1) } else { None },
        ) {
            (Some(Tok::Num(n, d)), _) if *d <= 2 => Some(*n),
            (_, Some(Tok::Num(n, d))) if *d <= 2 => Some(*n),
            _ => {
                let _ = day;
                None
            }
        };
        if let Some(d) = day.filter(|d| (1..=31).contains(d)) {
            start.day = Some(d as u8);
        }
    } else if year_at == 0 {
        // 2020/05/01, 2020-05, 2020/05//
        if let Some(&m) = nums_after.first() {
            if (1..=12).contains(&m) {
                start.month = Some(m as u8);
                if let Some(&d) = nums_after.get(1) {
                    if (1..=31).contains(&d) {
                        start.day = Some(d as u8);
                    } else {
                        lossy = true;
                    }
                }
            } else if (21..=24).contains(&m) {
                // EDTF seasons (21 spring .. 24 winter).
                lossy = true;
            } else {
                lossy = true;
            }
        }
    } else {
        // Day and month before the year: 01/05/2020 or 05/01/2020.
        let before: Vec<u32> = toks[..year_at]
            .iter()
            .filter_map(|t| match t {
                Tok::Num(n, d) if *d <= 2 => Some(*n),
                _ => None,
            })
            .collect();
        match before.as_slice() {
            [m] if (1..=12).contains(m) => start.month = Some(*m as u8),
            [a, b] => {
                let (a, b) = (*a, *b);
                if a > 12 && (1..=12).contains(&b) && a <= 31 {
                    start.month = Some(b as u8);
                    start.day = Some(a as u8);
                } else if b > 12 && (1..=12).contains(&a) && b <= 31 {
                    start.month = Some(a as u8);
                    start.day = Some(b as u8);
                } else if a == b && (1..=12).contains(&a) {
                    start.month = Some(a as u8);
                    start.day = Some(a as u8);
                } else {
                    lossy = true;
                }
            }
            [] => {}
            _ => lossy = true,
        }
    }
    date.start = Some(start);

    // Range: "2019-2020", "2020 Jan-Feb", "2020 May 1-15".
    if let Some(di) = toks.iter().position(|t| matches!(t, Tok::Dash)) {
        if di > year_at || month_tok.is_some_and(|m| di > m) {
            let mut end = start;
            let mut changed = false;
            for t in &toks[di + 1..] {
                match t {
                    Tok::Num(n, d) => {
                        if let Some(y) = plausible_year(*n, *d) {
                            if y == end.year {
                                continue;
                            }
                            end.year = y;
                            end.month = None;
                            end.day = None;
                            changed = true;
                        } else if *d <= 2 && (1..=31).contains(n) && start.day.is_some() {
                            end.day = Some(*n as u8);
                            changed = true;
                        } else if *d <= 2 && (1..=12).contains(n) && start.month.is_some() {
                            end.month = Some(*n as u8);
                            changed = true;
                        }
                    }
                    Tok::Month(m) => {
                        end.month = Some(*m);
                        end.day = None;
                        changed = true;
                    }
                    _ => {}
                }
            }
            if changed && end != start {
                date.end = Some(end);
            }
        }
    }
    if years.len() > 1 && date.end.is_none() {
        lossy = true;
    }
    if lossy {
        date.raw = Some(s.to_string());
    }
    Some(date)
}

fn parse_simple(s: &str) -> Option<Ymd> {
    let parts: Vec<&str> = s.trim().split('-').collect();
    if parts.len() > 3 || parts[0].len() != 4 {
        return None;
    }
    let num = |p: &str, len: usize| {
        (p.len() == len && p.bytes().all(|b| b.is_ascii_digit()))
            .then(|| p.parse::<u32>().ok())
            .flatten()
    };
    let year = num(parts[0], 4)? as i32;
    if !(1000..=2999).contains(&year) {
        return None;
    }
    let month = match parts.get(1) {
        Some(m) => Some(num(m, 2).filter(|m| (1..=12).contains(m))? as u8),
        None => None,
    };
    let day = match parts.get(2) {
        Some(d) => Some(num(d, 2).filter(|d| (1..=31).contains(d))? as u8),
        None => None,
    };
    Some(Ymd { year, month, day })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ymd(s: &str) -> (i32, Option<u8>, Option<u8>) {
        let d = parse_date(s).and_then(|d| d.start).unwrap_or(Ymd {
            year: 0,
            month: None,
            day: None,
        });
        (d.year, d.month, d.day)
    }

    #[test]
    fn common_forms() {
        assert_eq!(ymd("2020"), (2020, None, None));
        assert_eq!(ymd("2020/05/01"), (2020, Some(5), Some(1)));
        assert_eq!(ymd("2020/05//"), (2020, Some(5), None));
        assert_eq!(ymd("2020///"), (2020, None, None));
        assert_eq!(ymd("2020///Spring"), (2020, None, None));
        assert_eq!(ymd("2020-05-01"), (2020, Some(5), Some(1)));
        assert_eq!(ymd("2020 May 1"), (2020, Some(5), Some(1)));
        assert_eq!(ymd("1 May 2020"), (2020, Some(5), Some(1)));
        assert_eq!(ymd("May 2020"), (2020, Some(5), None));
        assert_eq!(ymd("2023/07/14"), (2023, Some(7), Some(14)));
        assert_eq!(ymd("14/07/2023"), (2023, Some(7), Some(14)));
        assert_eq!(ymd("2020 Jun 15"), (2020, Some(6), Some(15)));
    }

    #[test]
    fn ranges_and_odd_forms() {
        let d = parse_date("2019-2020").unwrap();
        assert_eq!(d.end.map(|e| e.year), Some(2020));
        let d = parse_date("2020 Jan-Feb").unwrap();
        assert_eq!(d.start.unwrap().month, Some(1));
        assert_eq!(d.end.unwrap().month, Some(2));
        let d = parse_date("2020-05-01/2020-06-30").unwrap();
        assert_eq!(d.end.unwrap().day, Some(30));
        let d = parse_date("c. 1850").unwrap();
        assert!(d.circa);
        let d = parse_date("Summer 2020").unwrap();
        assert_eq!(d.raw.as_deref(), Some("Summer 2020"));
        let d = parse_date("in press").unwrap();
        assert_eq!(d.literal.as_deref(), Some("in press"));
        assert!(parse_date("///").is_none());
        let d = parse_date("05/06/2020").unwrap();
        assert!(
            d.raw.is_some(),
            "ambiguous day and month must keep the source text"
        );
    }
}
