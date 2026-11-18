//! Text handling shared by every reader: decoding bytes, folding for keys and comparison.

use crate::diag::Diagnostic;
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// Decode input bytes to text with `\n` line endings.
///
/// UTF-8 and UTF-16 byte-order marks are honoured and removed. UTF-16 without a mark is
/// recognised by its zero bytes. Bytes that are not UTF-8 are read as Windows-1252, the
/// superset of Latin-1 that old Windows exports use, one byte at a time, so a UTF-8 file with
/// a few stray Latin-1 bytes keeps its UTF-8.
pub fn decode(bytes: &[u8], diags: &mut Vec<Diagnostic>) -> String {
    let text = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        diags.push(Diagnostic::info(
            "text.bom",
            "Removed a UTF-8 byte-order mark",
        ));
        decode_utf8_or_1252(rest, diags)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        diags.push(Diagnostic::info(
            "text.utf16",
            "Read UTF-16 (little-endian) text",
        ));
        decode_utf16(rest, false, diags)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        diags.push(Diagnostic::info(
            "text.utf16",
            "Read UTF-16 (big-endian) text",
        ));
        decode_utf16(rest, true, diags)
    } else if let Some(big_endian) = sniff_utf16(bytes) {
        diags.push(Diagnostic::info(
            "text.utf16",
            "Read UTF-16 text that had no byte-order mark",
        ));
        decode_utf16(bytes, big_endian, diags)
    } else {
        decode_utf8_or_1252(bytes, diags)
    };
    clean_text(text, diags)
}

/// Line endings to `\n`, and stray byte-order marks and NUL characters removed.
pub fn clean_text(text: String, diags: &mut Vec<Diagnostic>) -> String {
    let mut text = text;
    if let Some(rest) = text.strip_prefix('\u{FEFF}') {
        diags.push(Diagnostic::info("text.bom", "Removed a byte-order mark"));
        text = rest.to_string();
    }
    if !text.contains(['\r', '\u{FEFF}', '\0']) {
        return text;
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let (mut boms, mut nuls) = (0usize, 0usize);
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\u{FEFF}' => boms += 1,
            '\0' => nuls += 1,
            c => out.push(c),
        }
    }
    if boms > 0 {
        diags.push(Diagnostic::info(
            "text.bom",
            format!(
                "Removed {boms} byte-order mark(s) inside the text, as concatenated exports leave"
            ),
        ));
    }
    if nuls > 0 {
        diags.push(Diagnostic::warning(
            "text.nul",
            format!("Removed {nuls} NUL character(s)"),
        ));
    }
    out
}

fn sniff_utf16(bytes: &[u8]) -> Option<bool> {
    if bytes.len() < 4 {
        return None;
    }
    let sample = &bytes[..bytes.len().min(512) & !1];
    let even_zero = sample.iter().step_by(2).filter(|b| **b == 0).count();
    let odd_zero = sample
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|b| **b == 0)
        .count();
    let half = sample.len() / 2;
    if odd_zero * 10 >= half * 9 && even_zero == 0 {
        Some(false)
    } else if even_zero * 10 >= half * 9 && odd_zero == 0 {
        Some(true)
    } else {
        None
    }
}

fn decode_utf16(bytes: &[u8], big_endian: bool, diags: &mut Vec<Diagnostic>) -> String {
    let units = bytes.chunks(2).map(|c| {
        let pair = [c[0], *c.get(1).unwrap_or(&0)];
        if big_endian {
            u16::from_be_bytes(pair)
        } else {
            u16::from_le_bytes(pair)
        }
    });
    let mut bad = 0usize;
    let text: String = char::decode_utf16(units)
        .map(|r| {
            r.unwrap_or_else(|_| {
                bad += 1;
                '\u{FFFD}'
            })
        })
        .collect();
    if bad > 0 || bytes.len() % 2 == 1 {
        diags.push(Diagnostic::error(
            "text.bad-utf16",
            format!("{bad} broken UTF-16 sequence(s) became U+FFFD"),
        ));
    }
    text
}

fn decode_utf8_or_1252(bytes: &[u8], diags: &mut Vec<Diagnostic>) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let mut out = String::with_capacity(bytes.len() + 16);
    let mut rest = bytes;
    let mut foreign = 0usize;
    let mut first_line = None;
    let mut utf8_multibyte = false;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                utf8_multibyte |= s.bytes().any(|b| b >= 0x80);
                out.push_str(s);
                break;
            }
            Err(e) => {
                let (good, bad) = rest.split_at(e.valid_up_to());
                let good = std::str::from_utf8(good).unwrap_or_default();
                utf8_multibyte |= good.bytes().any(|b| b >= 0x80);
                out.push_str(good);
                let skip = e.error_len().unwrap_or(bad.len()).max(1);
                for &b in &bad[..skip.min(bad.len())] {
                    out.push(windows_1252(b));
                    foreign += 1;
                }
                if first_line.is_none() {
                    first_line = Some(out.matches('\n').count() + 1);
                }
                rest = &bad[skip.min(bad.len())..];
            }
        }
    }
    let mut d = if utf8_multibyte {
        Diagnostic::warning(
            "text.mixed-encoding",
            format!("Text is mostly UTF-8; read {foreign} other byte(s) as Windows-1252"),
        )
    } else {
        Diagnostic::info(
            "text.windows-1252",
            "Text is not UTF-8; read it as Windows-1252 (Latin-1)",
        )
    };
    if let Some(line) = first_line {
        d = d.at(line);
    }
    diags.push(d);
    out
}

/// One Windows-1252 byte as a character. The five unassigned bytes map to C1 controls.
pub fn windows_1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}',
        '\u{2021}', '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008D}',
        '\u{017D}', '\u{008F}', '\u{0090}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}',
        '\u{2022}', '\u{2013}', '\u{2014}', '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}',
        '\u{0153}', '\u{009D}', '\u{017E}', '\u{0178}',
    ];
    match b {
        0x80..=0x9F => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

/// Fold text to lower-case ASCII for citation keys: "Müller" gives "mueller", "Øresund"
/// "oresund", Greek and Cyrillic are transliterated. Characters with no ASCII form are kept.
pub fn fold_for_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if let Some(t) = special_fold(c) {
            out.push_str(t);
        } else if let Some(t) = transliterate(c) {
            out.push_str(t);
        } else {
            for d in c.to_string().nfkd() {
                if !is_combining_mark(d) {
                    out.extend(d.to_lowercase());
                }
            }
        }
    }
    out
}

/// Fold text for comparison: compatibility-decomposed, marks stripped, lower case, every run of
/// characters that are not letters or digits turned into one space.
pub fn fold_for_match(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = true;
    for c in s.nfkd() {
        if is_combining_mark(c) {
            continue;
        }
        let c = match c {
            'ß' => {
                out.push_str("ss");
                space = false;
                continue;
            }
            'æ' | 'Æ' => {
                out.push_str("ae");
                space = false;
                continue;
            }
            'œ' | 'Œ' => {
                out.push_str("oe");
                space = false;
                continue;
            }
            'ø' | 'Ø' => 'o',
            'ł' | 'Ł' => 'l',
            'đ' | 'Đ' => 'd',
            'ı' => 'i',
            c => c,
        };
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

fn special_fold(c: char) -> Option<&'static str> {
    Some(match c {
        'ä' | 'Ä' => "ae",
        'ö' | 'Ö' => "oe",
        'ü' | 'Ü' => "ue",
        'ß' | 'ẞ' => "ss",
        'æ' | 'Æ' => "ae",
        'œ' | 'Œ' => "oe",
        'ø' | 'Ø' => "o",
        'å' | 'Å' => "aa",
        'ł' | 'Ł' => "l",
        'đ' | 'Đ' => "d",
        'ð' | 'Ð' => "d",
        'þ' | 'Þ' => "th",
        'ı' => "i",
        _ => return None,
    })
}

fn transliterate(c: char) -> Option<&'static str> {
    let lower = c.to_lowercase().next().unwrap_or(c);
    Some(match lower {
        // Greek
        'α' | 'ά' => "a",
        'β' => "b",
        'γ' => "g",
        'δ' => "d",
        'ε' | 'έ' => "e",
        'ζ' => "z",
        'η' | 'ή' => "i",
        'θ' => "th",
        'ι' | 'ί' | 'ϊ' | 'ΐ' => "i",
        'κ' => "k",
        'λ' => "l",
        'μ' => "m",
        'ν' => "n",
        'ξ' => "x",
        'ο' | 'ό' => "o",
        'π' => "p",
        'ρ' => "r",
        'σ' | 'ς' => "s",
        'τ' => "t",
        'υ' | 'ύ' | 'ϋ' | 'ΰ' => "y",
        'φ' => "f",
        'χ' => "ch",
        'ψ' => "ps",
        'ω' | 'ώ' => "o",
        // Cyrillic
        'а' => "a",
        'б' => "b",
        'в' => "v",
        'г' => "g",
        'ґ' => "g",
        'д' => "d",
        'е' => "e",
        'ё' => "e",
        'є' => "ye",
        'ж' => "zh",
        'з' => "z",
        'и' => "i",
        'і' => "i",
        'ї' => "yi",
        'й' => "y",
        'к' => "k",
        'л' => "l",
        'м' => "m",
        'н' => "n",
        'о' => "o",
        'п' => "p",
        'р' => "r",
        'с' => "s",
        'т' => "t",
        'у' => "u",
        'ф' => "f",
        'х' => "kh",
        'ц' => "ts",
        'ч' => "ch",
        'ш' => "sh",
        'щ' => "shch",
        'ъ' => "",
        'ы' => "y",
        'ь' => "",
        'э' => "e",
        'ю' => "yu",
        'я' => "ya",
        _ => return None,
    })
}

/// Collapse runs of whitespace to one space and trim.
pub fn squash(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for word in s.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// Remove the inline HTML that CSL allows in titles (`<i>`, `<b>`, `<sup>`, `<sub>`,
/// `<span style="font-variant:small-caps;">`, `<span class="nocase">`). Other `<` stays.
pub fn strip_markup(s: &str) -> String {
    if !s.contains('<') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('<') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let end = tail.find('>');
        let tag = end.map(|e| &tail[1..e]);
        let known = tag.is_some_and(|t| {
            let name = t
                .trim_start_matches('/')
                .split([' ', '\t'])
                .next()
                .unwrap_or("");
            matches!(name, "i" | "b" | "sup" | "sub" | "sc" | "span")
        });
        if known {
            rest = &tail[end.unwrap() + 1..];
        } else {
            out.push('<');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

/// `Some(trimmed)` unless the text is empty after trimming.
pub fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// NFC form.
pub fn nfc(s: &str) -> String {
    s.nfc().collect()
}

/// Byte offset to 1-based line number.
pub(crate) struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub(crate) fn new(text: &str) -> LineIndex {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        LineIndex { starts }
    }

    pub(crate) fn line(&self, offset: usize) -> usize {
        match self.starts.binary_search(&offset) {
            Ok(i) => i + 1,
            Err(i) => i,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_like_the_old_key_makers() {
        assert_eq!(
            fold_for_key("Señor Øresund Straße"),
            "senor oresund strasse"
        );
        assert_eq!(fold_for_key("Müller"), "mueller");
        assert_eq!(fold_for_key("Достоевский"), "dostoevskiy");
        assert_eq!(fold_for_key("Σωκράτης"), "sokratis");
    }

    #[test]
    fn match_folding_drops_marks_and_punctuation() {
        assert_eq!(fold_for_match("Rodríguez,  J.-P."), "rodriguez j p");
        assert_eq!(fold_for_match("A Study: of  Moths!"), "a study of moths");
    }

    #[test]
    fn decodes_latin1_crlf_and_bom() {
        let mut d = Vec::new();
        assert_eq!(decode(b"\xEF\xBB\xBFa\r\nb\rc", &mut d), "a\nb\nc");
        let mut d = Vec::new();
        assert_eq!(decode(b"M\xfcller \x96 1", &mut d), "Müller – 1");
        assert_eq!(d[0].code, "text.windows-1252");
        let mut d = Vec::new();
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("TY  - ä".encode_utf16().flat_map(|u| u.to_le_bytes()))
            .collect();
        assert_eq!(decode(&utf16, &mut d), "TY  - ä");
        let mut d = Vec::new();
        assert_eq!(
            decode(
                "Müller \u{00e9}"
                    .as_bytes()
                    .iter()
                    .copied()
                    .chain([0xE9])
                    .collect::<Vec<_>>()
                    .as_slice(),
                &mut d
            ),
            "Müller éé"
        );
        assert_eq!(d[0].code, "text.mixed-encoding");
    }
}
