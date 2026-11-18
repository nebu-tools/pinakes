//! A small, forgiving XML reader: enough for PubMed's XML and nothing more.
//!
//! It builds a tree and never fails. Mismatched or unclosed tags, unknown entities and stray
//! `<` are reported with their line and repaired the way a browser would: an end tag closes
//! the nearest open element of that name, and unknown entities are kept as written. Namespaces,
//! DTDs and processing instructions are skipped.

use crate::diag::Diagnostic;
use crate::text::LineIndex;

#[derive(Debug, Default)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
    pub line: usize,
}

#[derive(Debug)]
pub enum Node {
    Element(Element),
    Text(String),
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|c| match c {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// Child elements with this name.
    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.elements().filter(move |e| e.name == name)
    }

    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    /// Follow a path of child names: `find("Journal/JournalIssue/Volume")`.
    pub fn find(&self, path: &str) -> Option<&Element> {
        let mut at = self;
        for part in path.split('/') {
            at = at.child(part)?;
        }
        Some(at)
    }

    /// All text below this element, with inline markup (`<i>`, `<sup>`) kept as CSL HTML
    /// when `markup` is set.
    pub fn text(&self, markup: bool) -> String {
        let mut out = String::new();
        self.collect_text(&mut out, markup);
        crate::text::squash(&out)
    }

    fn collect_text(&self, out: &mut String, markup: bool) {
        for c in &self.children {
            match c {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => {
                    let tag = match e.name.as_str() {
                        "i" | "b" | "sup" | "sub" if markup => Some(e.name.as_str()),
                        "u" | "sc" => None,
                        _ => None,
                    };
                    if let Some(t) = tag {
                        out.push_str(&format!("<{t}>"));
                        e.collect_text(out, markup);
                        out.push_str(&format!("</{t}>"));
                    } else {
                        e.collect_text(out, markup);
                    }
                }
            }
        }
    }

    pub fn text_of(&self, path: &str) -> Option<String> {
        self.find(path)
            .map(|e| e.text(false))
            .filter(|t| !t.is_empty())
    }
}

fn entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        _ => {
            let num = name.strip_prefix('#')?;
            let code = match num.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => num.parse().ok()?,
            };
            return char::from_u32(code);
        }
    })
}

fn unescape(s: &str, line: usize, diags: &mut Vec<Diagnostic>) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        match after.find(';').filter(|&e| e <= 10) {
            Some(end) => match entity(&after[..end]) {
                Some(c) => {
                    out.push(c);
                    rest = &after[end + 1..];
                }
                None => {
                    diags.push(
                        Diagnostic::warning(
                            "xml.unknown-entity",
                            format!("Unknown entity &{}; kept as written", &after[..end]),
                        )
                        .at(line),
                    );
                    out.push('&');
                    rest = after;
                }
            },
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Parse XML into a root element named `#document` whose children are the top-level nodes.
pub fn parse(src: &str, diags: &mut Vec<Diagnostic>) -> Element {
    let lines = LineIndex::new(src);
    let b = src.as_bytes();
    let mut stack: Vec<Element> = vec![Element {
        name: "#document".into(),
        line: 1,
        ..Element::default()
    }];
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            let start = i;
            while i < b.len() && b[i] != b'<' {
                i += 1;
            }
            let raw = &src[start..i];
            if stack.len() > 1 || !raw.trim().is_empty() {
                let t = unescape(raw, lines.line(start), diags);
                if stack.len() == 1 && !t.trim().is_empty() {
                    diags.push(
                        Diagnostic::warning("xml.stray-text", "Text outside the root element")
                            .at(lines.line(start)),
                    );
                }
                stack.last_mut().unwrap().children.push(Node::Text(t));
            }
            continue;
        }
        let rest = &src[i..];
        let skip_to = |pat: &str| rest.find(pat).map(|e| i + e + pat.len());
        if rest.starts_with("<!--") {
            i = skip_to("-->").unwrap_or(b.len());
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            let end = rest.find("]]>").map(|e| i + e).unwrap_or(b.len());
            let text = &src[i + 9..end.max(i + 9)];
            stack
                .last_mut()
                .unwrap()
                .children
                .push(Node::Text(text.to_string()));
            i = (end + 3).min(b.len());
            continue;
        }
        if rest.starts_with("<?") {
            i = skip_to("?>").unwrap_or(b.len());
            continue;
        }
        if rest.starts_with("<!") {
            // DOCTYPE, possibly with an internal subset in brackets.
            let mut depth = 0;
            let mut j = i + 2;
            while j < b.len() {
                match b[j] {
                    b'[' => depth += 1,
                    b']' => depth -= 1,
                    b'>' if depth <= 0 => break,
                    _ => {}
                }
                j += 1;
            }
            i = (j + 1).min(b.len());
            continue;
        }
        // A tag. Find its end, respecting quoted attribute values.
        let line = lines.line(i);
        let mut j = i + 1;
        let mut quote = 0u8;
        while j < b.len() {
            match b[j] {
                q @ (b'"' | b'\'') if quote == 0 => quote = q,
                q if q == quote => quote = 0,
                b'>' if quote == 0 => break,
                b'<' if quote == 0 => break,
                _ => {}
            }
            j += 1;
        }
        if j >= b.len() || b[j] == b'<' {
            diags.push(
                Diagnostic::error(
                    "xml.broken-tag",
                    "A tag is not closed with >; kept its text",
                )
                .at(line),
            );
            let t = unescape(&src[i..j.min(b.len())], line, diags);
            stack.last_mut().unwrap().children.push(Node::Text(t));
            i = j;
            continue;
        }
        let inner = &src[i + 1..j];
        i = j + 1;
        if let Some(name) = inner.strip_prefix('/') {
            let name = local(name.trim()).to_string();
            match stack.iter().rposition(|e| e.name == name) {
                Some(pos) if pos > 0 => {
                    while stack.len() > pos + 1 {
                        let open = stack.pop().unwrap();
                        diags.push(
                            Diagnostic::warning(
                                "xml.unclosed",
                                format!(
                                    "<{}> from line {} is not closed before </{name}>",
                                    open.name, open.line
                                ),
                            )
                            .at(line),
                        );
                        stack.last_mut().unwrap().children.push(Node::Element(open));
                    }
                    let done = stack.pop().unwrap();
                    stack.last_mut().unwrap().children.push(Node::Element(done));
                }
                _ => diags.push(
                    Diagnostic::warning(
                        "xml.stray-end",
                        format!("</{name}> closes nothing; ignored"),
                    )
                    .at(line),
                ),
            }
            continue;
        }
        let self_closing = inner.ends_with('/');
        let inner = inner.trim_end_matches('/');
        let name_end = inner
            .find(|c: char| c.is_whitespace())
            .unwrap_or(inner.len());
        let name = local(&inner[..name_end]).to_string();
        if name.is_empty() {
            diags.push(
                Diagnostic::warning("xml.broken-tag", "A < starts no tag; kept as text").at(line),
            );
            stack
                .last_mut()
                .unwrap()
                .children
                .push(Node::Text(format!("<{inner}>")));
            continue;
        }
        let mut el = Element {
            name,
            line,
            ..Element::default()
        };
        let mut a = &inner[name_end..];
        loop {
            a = a.trim_start();
            let Some(eq) = a.find('=') else { break };
            let key = local(a[..eq].trim()).to_string();
            let v = a[eq + 1..].trim_start();
            let Some(q) = v.chars().next().filter(|c| *c == '"' || *c == '\'') else {
                break;
            };
            let Some(end) = v[1..].find(q) else { break };
            el.attrs.push((key, unescape(&v[1..1 + end], line, diags)));
            a = &v[end + 2..];
        }
        if self_closing {
            stack.last_mut().unwrap().children.push(Node::Element(el));
        } else {
            stack.push(el);
        }
    }
    if stack.len() > 1 {
        // One cut-off leaves every enclosing element open; report it once, at the innermost.
        let names: Vec<String> = stack[1..].iter().map(|e| format!("<{}>", e.name)).collect();
        let innermost = stack.last().map(|e| e.line).unwrap_or(1);
        diags.push(
            Diagnostic::error(
                "xml.unclosed",
                format!(
                    "The file ends with {} still open; it may be cut off",
                    names.join(" ")
                ),
            )
            .at(innermost),
        );
    }
    while stack.len() > 1 {
        let open = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(Node::Element(open));
    }
    stack.pop().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_and_repairs() {
        let mut d = Vec::new();
        let doc = parse("<?xml version=\"1.0\"?>\n<!DOCTYPE x [ <!ENTITY a \"b\"> ]>\n<a k='v &amp; w'><b>x &lt; <i>y</i></b><c/><d>open</a>", &mut d);
        let a = doc.child("a").unwrap();
        assert_eq!(a.attr("k"), Some("v & w"));
        assert_eq!(a.child("b").unwrap().text(true), "x < <i>y</i>");
        assert!(a.child("c").is_some());
        assert_eq!(a.text_of("d").as_deref(), Some("open"));
        assert!(d.iter().any(|x| x.code == "xml.unclosed"));
    }

    #[test]
    fn never_panics_on_fragments() {
        for s in [
            "<",
            "<a",
            "</a>",
            "<a b='>",
            "&",
            "&#xZZ;",
            "<![CDATA[",
            "<!--",
            "<a><b></a></b>",
            "<?",
            "<!",
        ] {
            let _ = parse(s, &mut Vec::new());
        }
    }
}
