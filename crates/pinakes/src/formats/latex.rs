//! LaTeX in BibTeX field values, to and from Unicode text with CSL's small HTML subset
//! (`<i>`, `<b>`, `<sup>`, `<sub>` and a small-caps span).

use unicode_normalization::UnicodeNormalization;

fn accent(cmd: &str) -> Option<char> {
    Some(match cmd {
        "'" => '\u{0301}',
        "`" => '\u{0300}',
        "^" => '\u{0302}',
        "\"" => '\u{0308}',
        "~" => '\u{0303}',
        "=" => '\u{0304}',
        "." => '\u{0307}',
        "u" => '\u{0306}',
        "v" => '\u{030C}',
        "H" => '\u{030B}',
        "c" => '\u{0327}',
        "k" => '\u{0328}',
        "r" => '\u{030A}',
        "d" => '\u{0323}',
        "b" => '\u{0331}',
        "t" => '\u{0361}',
        _ => return None,
    })
}

fn symbol(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "ss" => "ß",
        "SS" => "SS",
        "ae" => "æ",
        "AE" => "Æ",
        "oe" => "œ",
        "OE" => "Œ",
        "aa" => "å",
        "AA" => "Å",
        "o" => "ø",
        "O" => "Ø",
        "l" => "ł",
        "L" => "Ł",
        "i" => "ı",
        "j" => "ȷ",
        "dh" => "ð",
        "DH" => "Ð",
        "th" => "þ",
        "TH" => "Þ",
        "ng" => "ŋ",
        "NG" => "Ŋ",
        "dj" => "đ",
        "DJ" => "Đ",
        "textendash" => "–",
        "textemdash" => "—",
        "textquoteleft" => "‘",
        "textquoteright" => "’",
        "textquotedblleft" => "“",
        "textquotedblright" => "”",
        "guillemotleft" | "guillemetleft" => "«",
        "guillemotright" | "guillemetright" => "»",
        "textregistered" => "®",
        "texttrademark" => "™",
        "textcopyright" | "copyright" => "©",
        "textbackslash" => "\\",
        "textdollar" => "$",
        "textunderscore" => "_",
        "textasciitilde" => "~",
        "textasciicircum" => "^",
        "textless" => "<",
        "textgreater" => ">",
        "textbar" => "|",
        "textbullet" => "•",
        "textperiodcentered" => "·",
        "textdegree" | "degree" => "°",
        "textsection" | "S" => "§",
        "textparagraph" | "P" => "¶",
        "pounds" | "textsterling" => "£",
        "euro" | "texteuro" => "€",
        "textmu" => "µ",
        "ldots" | "dots" | "textellipsis" => "…",
        "slash" => "/",
        "textpm" | "pm" => "±",
        "times" => "×",
        "div" => "÷",
        "geq" | "ge" => "≥",
        "leq" | "le" => "≤",
        "neq" | "ne" => "≠",
        "approx" => "≈",
        "infty" => "∞",
        "rightarrow" | "to" => "→",
        "leftarrow" => "←",
        "circ" => "∘",
        "prime" => "′",
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" | "varepsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" | "vartheta" => "θ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "rho" => "ρ",
        "sigma" => "σ",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" | "varphi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Upsilon" => "Υ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        "quad" | "qquad" | "enspace" | "thinspace" => " ",
        _ => return None,
    })
}

fn markup(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "textit" | "emph" | "textsl" | "mathit" | "it" | "em" => "i",
        "textbf" | "mathbf" | "bf" => "b",
        "textsuperscript" => "sup",
        "textsubscript" => "sub",
        "textsc" => "sc",
        _ => return None,
    })
}

/// Commands whose argument is kept as plain text.
fn passthrough(cmd: &str) -> bool {
    matches!(
        cmd,
        "textrm"
            | "textsf"
            | "texttt"
            | "textnormal"
            | "mbox"
            | "hbox"
            | "text"
            | "mathrm"
            | "mathsf"
            | "mathtt"
            | "textup"
            | "textmd"
            | "ensuremath"
            | "url"
            | "nolinkurl"
            | "NoCaseChange"
            | "MakeUppercase"
            | "uppercase"
            | "MakeLowercase"
            | "lowercase"
            | "href"
            | "foreignlanguage"
            | "hyphenation"
    )
}

struct Parser<'a> {
    s: &'a [char],
    i: usize,
}

impl Parser<'_> {
    fn skip_spaces(&mut self) {
        while self.s.get(self.i) == Some(&' ') {
            self.i += 1;
        }
    }

    /// One argument: a braced group (its LaTeX converted) or one character.
    fn argument(&mut self) -> String {
        self.skip_spaces();
        match self.s.get(self.i) {
            Some('{') => {
                let start = self.i + 1;
                let mut depth = 0usize;
                while self.i < self.s.len() {
                    match self.s[self.i] {
                        '\\' => self.i += 1,
                        '{' => depth += 1,
                        '}' => {
                            depth = depth.saturating_sub(1);
                            if depth == 0 {
                                self.i += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    self.i += 1;
                }
                let end = self.i.saturating_sub(1).max(start).min(self.s.len());
                let inner: String = self.s[start..end].iter().collect();
                convert(&inner)
            }
            Some('\\') => {
                // \"\i, \'{\i}
                let start = self.i;
                self.i += 1;
                while self.s.get(self.i).is_some_and(|c| c.is_ascii_alphabetic()) {
                    self.i += 1;
                }
                let inner: String = self.s[start..self.i].iter().collect();
                convert(&inner)
            }
            Some(&c) => {
                self.i += 1;
                c.to_string()
            }
            None => String::new(),
        }
    }
}

fn with_accent(base: String, mark: char) -> String {
    let mut chars = base.chars();
    match chars.next() {
        Some(first) => {
            // An accent over a dotless i or j is an accent over i or j.
            let first = match first {
                'ı' => 'i',
                'ȷ' => 'j',
                c => c,
            };
            let mut out: String = [first, mark].iter().collect::<String>().nfc().collect();
            out.extend(chars);
            out
        }
        None => mark.to_string(),
    }
}

fn convert(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut p = Parser { s: &chars, i: 0 };
    let mut out = String::with_capacity(src.len());
    let mut math = false;
    while p.i < chars.len() {
        let c = chars[p.i];
        match c {
            '{' | '}' => p.i += 1,
            '~' if !math => {
                out.push('\u{00A0}');
                p.i += 1;
            }
            '-' if chars[p.i..].starts_with(&['-', '-', '-']) => {
                out.push('—');
                p.i += 3;
            }
            '-' if chars[p.i..].starts_with(&['-', '-']) => {
                out.push('–');
                p.i += 2;
            }
            '`' if chars[p.i..].starts_with(&['`', '`']) => {
                out.push('“');
                p.i += 2;
            }
            '\'' if chars[p.i..].starts_with(&['\'', '\'']) => {
                out.push('”');
                p.i += 2;
            }
            '$' => {
                math = !math;
                p.i += 1;
            }
            '^' | '_' if math => {
                p.i += 1;
                let arg = p.argument();
                let tag = if c == '^' { "sup" } else { "sub" };
                out.push_str(&format!("<{tag}>{arg}</{tag}>"));
            }
            '%' => {
                // A comment runs to the end of the line.
                while p.i < chars.len() && chars[p.i] != '\n' {
                    p.i += 1;
                }
            }
            '\\' => {
                p.i += 1;
                let start = p.i;
                while p.i < chars.len() && chars[p.i].is_ascii_alphabetic() {
                    p.i += 1;
                }
                let name: String = if p.i == start {
                    p.i += 1;
                    chars.get(start).map(|c| c.to_string()).unwrap_or_default()
                } else {
                    chars[start..p.i].iter().collect()
                };
                let alphabetic = name.chars().all(|c| c.is_ascii_alphabetic());
                if let Some(mark) = accent(&name) {
                    // \c c needs the space; \"u does not.
                    let arg = p.argument();
                    out.push_str(&with_accent(arg, mark));
                } else if let Some(sym) = symbol(&name) {
                    if alphabetic {
                        if chars.get(p.i) == Some(&' ') {
                            p.i += 1;
                        } else if chars[p.i..].starts_with(&['{', '}']) {
                            p.i += 2;
                        }
                    }
                    out.push_str(sym);
                } else if let Some(tag) = markup(&name) {
                    let inner = p.argument();
                    if tag == "sc" {
                        out.push_str(&format!(
                            "<span style=\"font-variant:small-caps;\">{inner}</span>"
                        ));
                    } else {
                        out.push_str(&format!("<{tag}>{inner}</{tag}>"));
                    }
                } else if !alphabetic {
                    match name.as_str() {
                        "\\" => out.push(' '),
                        " " | "," | ";" | ":" | "!" => out.push(' '),
                        "-" | "/" => {}
                        other => out.push_str(other),
                    }
                } else if passthrough(&name) {
                    if name == "href" {
                        let _url = p.argument();
                    }
                    if name == "foreignlanguage" {
                        let _lang = p.argument();
                    }
                    let inner = p.argument();
                    out.push_str(&inner);
                } else {
                    // Unknown command: keep its argument, drop the command.
                    p.skip_spaces();
                    if chars.get(p.i) == Some(&'{') {
                        out.push_str(&p.argument());
                    }
                }
            }
            c => {
                out.push(c);
                p.i += 1;
            }
        }
    }
    out
}

/// LaTeX to text: accents composed, symbols and dashes as Unicode, markup as CSL HTML, braces
/// dropped, whitespace squashed.
pub fn latex_to_text(s: &str) -> String {
    if !s.contains(['\\', '{', '}', '~', '$', '-', '`', '\'', '%']) {
        return crate::text::squash(s);
    }
    let converted = convert(s);
    let squashed: String = crate::text::squash(&converted.replace('\u{00A0}', " "));
    squashed.nfc().collect()
}

fn reverse_accent(mark: char) -> Option<&'static str> {
    Some(match mark {
        '\u{0301}' => "'",
        '\u{0300}' => "`",
        '\u{0302}' => "^",
        '\u{0308}' => "\"",
        '\u{0303}' => "~",
        '\u{0304}' => "=",
        '\u{0307}' => ".",
        '\u{0306}' => "u",
        '\u{030C}' => "v",
        '\u{030B}' => "H",
        '\u{0327}' => "c",
        '\u{0328}' => "k",
        '\u{030A}' => "r",
        '\u{0323}' => "d",
        _ => return None,
    })
}

fn reverse_symbol(c: char) -> Option<&'static str> {
    Some(match c {
        'ß' => "{\\ss}",
        'æ' => "{\\ae}",
        'Æ' => "{\\AE}",
        'œ' => "{\\oe}",
        'Œ' => "{\\OE}",
        'å' => "{\\aa}",
        'Å' => "{\\AA}",
        'ø' => "{\\o}",
        'Ø' => "{\\O}",
        'ł' => "{\\l}",
        'Ł' => "{\\L}",
        'ı' => "{\\i}",
        'ð' => "{\\dh}",
        'þ' => "{\\th}",
        '–' => "--",
        '—' => "---",
        '‘' => "`",
        '’' => "'",
        '“' => "``",
        '”' => "''",
        '…' => "\\ldots{}",
        '§' => "\\S{}",
        '©' => "\\textcopyright{}",
        '®' => "\\textregistered{}",
        '°' => "\\textdegree{}",
        '£' => "\\pounds{}",
        '€' => "\\euro{}",
        '±' => "$\\pm$",
        '×' => "$\\times$",
        '≤' => "$\\leq$",
        '≥' => "$\\geq$",
        'α' => "$\\alpha$",
        'β' => "$\\beta$",
        'γ' => "$\\gamma$",
        'δ' => "$\\delta$",
        'κ' => "$\\kappa$",
        'λ' => "$\\lambda$",
        'μ' | 'µ' => "$\\mu$",
        'π' => "$\\pi$",
        'σ' => "$\\sigma$",
        'ω' => "$\\omega$",
        _ => return None,
    })
}

/// Text to a BibTeX value. With `ascii`, letters outside ASCII become LaTeX accents and
/// symbols, for classic BibTeX; without it, Unicode is kept, for biber. `protect` wraps words
/// with capitals after the first letter (acronyms, "iPhone") in braces so styles keep their
/// case.
pub fn text_to_latex(s: &str, ascii: bool, protect: bool) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    let s = html_to_latex(s);
    let words = split_keep(&s);
    for w in words {
        let needs_protect = protect
            && w.chars().skip(1).any(|c| c.is_uppercase())
            && w.chars().any(|c| c.is_alphabetic())
            && !w.contains('\\');
        if needs_protect {
            out.push('{');
        }
        for c in w.chars() {
            match c {
                '&' | '%' | '$' | '#' | '_' if !w.starts_with('\\') => {
                    out.push('\\');
                    out.push(c);
                }
                '\u{1}' => out.push('\\'),
                '\u{2}' => out.push('{'),
                '\u{3}' => out.push('}'),
                '\u{4}' => out.push('$'),
                '{' => out.push_str("\\{"),
                '}' => out.push_str("\\}"),
                '\\' => out.push_str("\\textbackslash{}"),
                '~' => out.push_str("\\textasciitilde{}"),
                '^' => out.push_str("\\textasciicircum{}"),
                c if ascii && !c.is_ascii() => out.push_str(&encode_char(c)),
                c => out.push(c),
            }
        }
        if needs_protect {
            out.push('}');
        }
    }
    out
}

fn encode_char(c: char) -> String {
    if let Some(sym) = reverse_symbol(c) {
        return sym.to_string();
    }
    let decomposed: Vec<char> = c.to_string().nfd().collect();
    if decomposed.len() == 2 && decomposed[0].is_ascii() {
        if let Some(cmd) = reverse_accent(decomposed[1]) {
            let base = if decomposed[0] == 'i' {
                "\\i".to_string()
            } else {
                decomposed[0].to_string()
            };
            return if cmd.chars().all(|c| c.is_ascii_alphabetic()) {
                format!("{{\\{cmd}{{{base}}}}}")
            } else {
                format!("{{\\{cmd}{base}}}")
            };
        }
    }
    c.to_string()
}

/// Turn CSL HTML into LaTeX commands, marking LaTeX syntax with control characters so the
/// escaping pass leaves it alone: \u{1} is a backslash, \u{2} and \u{3} braces.
fn html_to_latex(s: &str) -> String {
    if !s.contains('<') {
        return s.to_string();
    }
    let mut out = s.to_string();
    for (open, close, cmd) in [
        ("<i>", "</i>", "textit"),
        ("<em>", "</em>", "emph"),
        ("<b>", "</b>", "textbf"),
        ("<strong>", "</strong>", "textbf"),
        ("<sup>", "</sup>", "textsuperscript"),
        ("<sub>", "</sub>", "textsubscript"),
        (
            "<span style=\"font-variant:small-caps;\">",
            "</span>",
            "textsc",
        ),
        ("<span class=\"nocase\">", "</span>", ""),
    ] {
        while let Some(a) = out.find(open) {
            let Some(rel) = out[a + open.len()..].find(close) else {
                break;
            };
            let b = a + open.len() + rel;
            let inner = out[a + open.len()..b].to_string();
            let replacement = if cmd.is_empty() {
                format!("\u{2}{inner}\u{3}")
            } else {
                format!("\u{1}{cmd}\u{2}{inner}\u{3}")
            };
            out.replace_range(a..b + close.len(), &replacement);
        }
    }
    out
}

fn split_keep(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_whitespace() || matches!(c, '\u{2}' | '\u{3}') {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.push(c.to_string());
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_and_symbols() {
        assert_eq!(
            latex_to_text(r#"M{\"u}ller and Stra\ss{}e"#),
            "Müller and Straße"
        );
        assert_eq!(
            latex_to_text(r#"\'{e}t\'e -- {DNA} \textit{in situ}"#),
            "été – DNA <i>in situ</i>"
        );
        assert_eq!(latex_to_text(r"10~km \& more"), "10 km & more");
        assert_eq!(
            latex_to_text(r#"\c{c}a \c c \v{s} \k{a} \"{\i}"#),
            "ça ç š ą ï"
        );
        assert_eq!(
            latex_to_text(r"H$_2$O and $\alpha$-decay"),
            "H<sub>2</sub>O and α-decay"
        );
        assert_eq!(
            latex_to_text(r"\textsc{Unesco} \url{https://example.org/a_b}"),
            "<span style=\"font-variant:small-caps;\">Unesco</span> https://example.org/a_b"
        );
        assert_eq!(latex_to_text(r"``quoted'' text"), "“quoted” text");
    }

    #[test]
    fn writing() {
        assert_eq!(
            text_to_latex("Müller & Søn", true, false),
            r#"M{\"u}ller \& S{\o}n"#
        );
        assert_eq!(
            text_to_latex("Müller & Søn", false, false),
            r"Müller \& Søn"
        );
        assert_eq!(
            text_to_latex("The DNA of <i>Bombyx</i>", false, true),
            r"The {DNA} of \textit{Bombyx}"
        );
        assert_eq!(text_to_latex("ça", true, false), r"{\c{c}}a");
    }

    #[test]
    fn unbalanced_input_does_not_panic() {
        for s in [
            "{",
            "}",
            "\\",
            "\\\"",
            "$",
            "\\textit{",
            "{\\\"",
            "^",
            "--",
            "\\c",
        ] {
            let _ = latex_to_text(s);
        }
    }
}
