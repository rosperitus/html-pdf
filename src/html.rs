//! HTML to DOM.
//!
//! A forgiving subset of the WHATWG algorithm: tags, attributes, character
//! references, comments, raw-text elements and the implied end tags that
//! matter for documents (`p`, `li`, `dt`/`dd`, table parts). Any input
//! produces a tree; nothing here fails.

use std::borrow::Cow;

pub struct Dom {
    /// Arena in document order: a parent always precedes its children.
    /// Node 0 is the document itself.
    pub nodes: Vec<Node>,
}

pub struct Node {
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub data: NodeData,
}

pub enum NodeData {
    Document,
    Element(Element),
    Text(String),
}

pub struct Element {
    /// Lowercase tag name.
    pub tag: String,
    /// Lowercase names; first occurrence wins.
    pub attrs: Vec<(String, String)>,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

impl Dom {
    pub fn element(&self, id: usize) -> Option<&Element> {
        match &self.nodes.get(id)?.data {
            NodeData::Element(el) => Some(el),
            _ => None,
        }
    }
}

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];
const RAW_TEXT: &[&str] = &["script", "style", "title", "textarea", "xmp"];
/// Start tags that close an open `p`.
const CLOSES_P: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "center",
    "dd",
    "details",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul",
];
/// Elements an implied end tag never looks past.
const SCOPE: &[&str] = &["table", "td", "th", "caption", "html", "button"];

pub fn parse(src: &str) -> Dom {
    let mut b = Builder {
        dom: Dom {
            nodes: vec![Node {
                parent: None,
                children: Vec::new(),
                data: NodeData::Document,
            }],
        },
        stack: vec![0],
    };
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < src.len() {
        let Some(off) = src[i..].find('<') else {
            b.text(&src[i..]);
            break;
        };
        b.text(&src[i..i + off]);
        i += off;
        let rest = &bytes[i..];
        i = if rest.starts_with(b"<!--") {
            find(bytes, i + 4, b"-->").map_or(src.len(), |e| e + 3)
        } else if rest.starts_with(b"<!") || rest.starts_with(b"<?") {
            find(bytes, i, b">").map_or(src.len(), |e| e + 1)
        } else if rest.starts_with(b"</") && rest.get(2).is_some_and(u8::is_ascii_alphabetic) {
            let (name, end) = tag_name(src, i + 2);
            b.end(&name);
            find(bytes, end, b">").map_or(src.len(), |e| e + 1)
        } else if rest.get(1).is_some_and(u8::is_ascii_alphabetic) {
            start_tag(src, i + 1, &mut b)
        } else {
            b.text("<");
            i + 1
        };
    }
    b.dom
}

/// Parses a start tag whose name begins at `i`; returns the index after it.
fn start_tag(src: &str, i: usize, b: &mut Builder) -> usize {
    let bytes = src.as_bytes();
    let (tag, mut i) = tag_name(src, i);
    let mut attrs: Vec<(String, String)> = Vec::new();
    loop {
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) || bytes.get(i) == Some(&b'/') {
            i += 1;
        }
        match bytes.get(i) {
            None => break,
            Some(b'>') => {
                i += 1;
                break;
            }
            Some(_) => {}
        }
        let start = i;
        while bytes
            .get(i)
            .is_some_and(|c| !c.is_ascii_whitespace() && !b"=>/".contains(c))
        {
            i += 1;
        }
        // A stray '=' with no name: skip it so the loop advances.
        let name = if i == start {
            i += 1;
            String::new()
        } else {
            src[start..i].to_ascii_lowercase()
        };
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        let mut value = String::new();
        if bytes.get(i) == Some(&b'=') {
            i += 1;
            while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                i += 1;
            }
            let raw = match bytes.get(i) {
                Some(&q @ (b'"' | b'\'')) => {
                    let end = find(bytes, i + 1, &[q]).unwrap_or(src.len());
                    let raw = &src[i + 1..end];
                    i = (end + 1).min(src.len());
                    raw
                }
                _ => {
                    let start = i;
                    while bytes
                        .get(i)
                        .is_some_and(|c| !c.is_ascii_whitespace() && *c != b'>')
                    {
                        i += 1;
                    }
                    &src[start..i]
                }
            };
            value = decode(raw).into_owned();
        }
        if !name.is_empty() && !attrs.iter().any(|(n, _)| *n == name) {
            attrs.push((name, value));
        }
    }

    let raw_text = RAW_TEXT.contains(&tag.as_str());
    let close = format!("</{tag}");
    b.start(tag, attrs);
    if raw_text {
        let end = find_ci(bytes, i, close.as_bytes()).unwrap_or(src.len());
        b.raw_text(&src[i..end]);
        b.pop();
        i = find(bytes, end, b">").map_or(src.len(), |e| e + 1);
    }
    i
}

/// Reads a tag name starting at `i`; returns it lowercased and the index after it.
fn tag_name(src: &str, i: usize) -> (String, usize) {
    let bytes = src.as_bytes();
    let mut end = i;
    while bytes
        .get(end)
        .is_some_and(|c| !c.is_ascii_whitespace() && !b"/>".contains(c))
    {
        end += 1;
    }
    (src[i..end].to_ascii_lowercase(), end)
}

fn find(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

fn find_ci(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle))
        .map(|p| p + from)
}

struct Builder {
    dom: Dom,
    /// Open elements; the document node at the bottom is never popped.
    stack: Vec<usize>,
}

impl Builder {
    fn current(&self) -> usize {
        self.stack.last().copied().unwrap_or(0)
    }

    fn tag(&self, id: usize) -> &str {
        self.dom.element(id).map_or("", |el| el.tag.as_str())
    }

    fn push_node(&mut self, data: NodeData) -> usize {
        let id = self.dom.nodes.len();
        let parent = self.current();
        self.dom.nodes.push(Node {
            parent: Some(parent),
            children: Vec::new(),
            data,
        });
        if let Some(p) = self.dom.nodes.get_mut(parent) {
            p.children.push(id);
        }
        id
    }

    fn pop(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }

    /// Pops up to and including the nearest open element in `targets`, unless
    /// an element in `stop` comes first.
    fn close_nearest(&mut self, targets: &[&str], stop: &[&str]) {
        for pos in (1..self.stack.len()).rev() {
            let tag = self.tag(self.stack[pos]);
            if targets.contains(&tag) {
                self.stack.truncate(pos);
                return;
            }
            if stop.contains(&tag) {
                return;
            }
        }
    }

    fn start(&mut self, tag: String, attrs: Vec<(String, String)>) {
        match tag.as_str() {
            "li" => self.close_nearest(&["li"], &["ul", "ol", "table", "td", "th"]),
            "dt" | "dd" => self.close_nearest(&["dt", "dd"], &["dl", "table", "td", "th"]),
            "td" | "th" => self.close_nearest(&["td", "th"], &["tr", "table"]),
            "tr" => self.close_nearest(&["tr"], &["table"]),
            "thead" | "tbody" | "tfoot" => {
                self.close_nearest(&["thead", "tbody", "tfoot"], &["table"])
            }
            _ => {}
        }
        if CLOSES_P.contains(&tag.as_str()) {
            self.close_nearest(&["p"], SCOPE);
        }
        let void = VOID.contains(&tag.as_str());
        let id = self.push_node(NodeData::Element(Element { tag, attrs }));
        if !void {
            self.stack.push(id);
        }
    }

    fn end(&mut self, tag: &str) {
        self.close_nearest(&[tag], &[]);
    }

    fn text(&mut self, raw: &str) {
        if !raw.is_empty() {
            self.raw_text(&decode(raw));
        }
    }

    fn raw_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let parent = self.current();
        let last = self
            .dom
            .nodes
            .get(parent)
            .and_then(|p| p.children.last().copied());
        if let Some(NodeData::Text(t)) = last
            .and_then(|l| self.dom.nodes.get_mut(l))
            .map(|n| &mut n.data)
        {
            t.push_str(text);
        } else {
            self.push_node(NodeData::Text(text.to_owned()));
        }
    }
}

/// Decodes character references. Unknown ones stay as written.
pub fn decode(s: &str) -> Cow<'_, str> {
    if !s.contains('&') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        match reference(rest) {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// Parses a reference at the start of `s` (which begins with '&'); returns
/// the character and the number of bytes consumed.
fn reference(s: &str) -> Option<(char, usize)> {
    // Bounded search keeps text with many bare '&' linear.
    let semi = s.bytes().take(32).position(|b| b == b';')?;
    let body = &s[1..semi];
    let c = if let Some(num) = body.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        match code {
            0 => '\u{FFFD}',
            _ => char::from_u32(code).unwrap_or('\u{FFFD}'),
        }
    } else {
        ENTITIES.iter().find(|(n, _)| *n == body)?.1
    };
    Some((c, semi + 1))
}

// ponytail: the common subset of the ~2200 named references; add more when documents need them
const ENTITIES: &[(&str, char)] = &[
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("quot", '"'),
    ("apos", '\''),
    ("nbsp", '\u{A0}'),
    ("shy", '\u{AD}'),
    ("copy", '©'),
    ("reg", '®'),
    ("trade", '™'),
    ("hellip", '…'),
    ("mdash", '—'),
    ("ndash", '–'),
    ("minus", '−'),
    ("laquo", '«'),
    ("raquo", '»'),
    ("lsquo", '‘'),
    ("rsquo", '’'),
    ("sbquo", '‚'),
    ("ldquo", '“'),
    ("rdquo", '”'),
    ("bdquo", '„'),
    ("bull", '•'),
    ("middot", '·'),
    ("deg", '°'),
    ("plusmn", '±'),
    ("times", '×'),
    ("divide", '÷'),
    ("euro", '€'),
    ("pound", '£'),
    ("yen", '¥'),
    ("cent", '¢'),
    ("sect", '§'),
    ("para", '¶'),
    ("numero", '№'),
    ("micro", 'µ'),
    ("frac12", '½'),
    ("frac14", '¼'),
    ("frac34", '¾'),
    ("sup1", '¹'),
    ("sup2", '²'),
    ("sup3", '³'),
    ("larr", '←'),
    ("rarr", '→'),
    ("uarr", '↑'),
    ("darr", '↓'),
    ("harr", '↔'),
    ("ensp", '\u{2002}'),
    ("emsp", '\u{2003}'),
    ("thinsp", '\u{2009}'),
    ("zwnj", '\u{200C}'),
    ("zwj", '\u{200D}'),
    ("lrm", '\u{200E}'),
    ("rlm", '\u{200F}'),
    ("iexcl", '¡'),
    ("iquest", '¿'),
    ("prime", '′'),
    ("Prime", '″'),
    ("infin", '∞'),
    ("ne", '≠'),
    ("le", '≤'),
    ("ge", '≥'),
    ("asymp", '≈'),
    ("check", '✓'),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes the tree as `tag(children)` with text in quotes.
    fn dump(dom: &Dom, id: usize) -> String {
        let node = &dom.nodes[id];
        let kids: Vec<String> = node.children.iter().map(|&c| dump(dom, c)).collect();
        match &node.data {
            NodeData::Document => kids.join(" "),
            NodeData::Text(t) => format!("{t:?}"),
            NodeData::Element(el) if kids.is_empty() => el.tag.clone(),
            NodeData::Element(el) => format!("{}({})", el.tag, kids.join(" ")),
        }
    }

    fn tree(src: &str) -> String {
        dump(&parse(src), 0)
    }

    #[test]
    fn nesting_and_text() {
        assert_eq!(tree("<p>a <B>b</b> c</p>"), r#"p("a " b("b") " c")"#);
        assert_eq!(tree("x<br>y<hr/>z"), r#""x" br "y" hr "z""#);
        assert_eq!(tree("<div>a</span>b</div>"), r#"div("ab")"#);
        assert_eq!(tree("<!DOCTYPE html><!-- c --><p>a</p>"), r#"p("a")"#);
    }

    #[test]
    fn implied_end_tags() {
        assert_eq!(tree("<p>one<div>two</div>"), r#"p("one") div("two")"#);
        assert_eq!(tree("<p><b>one<h1>t</h1>"), r#"p(b("one")) h1("t")"#);
        assert_eq!(tree("<ul><li>a<li>b</ul>"), r#"ul(li("a") li("b"))"#);
        assert_eq!(
            tree("<ul><li>a<ul><li>b</ul><li>c</ul>"),
            r#"ul(li("a" ul(li("b"))) li("c"))"#
        );
        assert_eq!(
            tree("<table><tr><td>1<td>2<tr><td>3</table>x"),
            r#"table(tr(td("1") td("2")) tr(td("3"))) "x""#
        );
        assert_eq!(tree("<td><p>a<table></table>"), r#"td(p("a") table)"#);
    }

    #[test]
    fn attributes() {
        let dom = parse(r#"<A HREF="x&amp;y" class='a b' checked id=main data-x = 1 href=dup>"#);
        let el = dom.element(1).unwrap();
        assert_eq!(el.tag, "a");
        assert_eq!(
            el.attrs,
            [
                ("href", "x&y"),
                ("class", "a b"),
                ("checked", ""),
                ("id", "main"),
                ("data-x", "1")
            ]
            .map(|(n, v)| (n.to_string(), v.to_string()))
        );
    }

    #[test]
    fn raw_text_elements() {
        assert_eq!(
            tree("<style>p > b { x: '</p>' }</STYLE>a"),
            r#"style("p > b { x: '</p>' }") "a""#
        );
        assert_eq!(
            tree("<script>if (a<b) {}</script>"),
            r#"script("if (a<b) {}")"#
        );
    }

    #[test]
    fn character_references() {
        assert_eq!(
            decode("&amp;&lt;&#1071;&#x42F;&nbsp;&bogus; & &#0; &#xD800; &#99999999999;&numero;"),
            "&<ЯЯ\u{A0}&bogus; & \u{FFFD} \u{FFFD} &#99999999999;№"
        );
        assert_eq!(tree("a < b"), r#""a < b""#);
    }

    #[test]
    fn never_panics() {
        let src = "<!DOCTYPE html><html><head><style>p{}</style></head><body a='б' b=\"в\" c=г>\
                   <p>Привет &amp; &#x42F;<br/><!-- c --><ul><li>x</ul><table><tr><td>т</table>\
                   <script>a<b</script><? pi ?></body></html>";
        for end in (0..=src.len()).filter(|&e| src.is_char_boundary(e)) {
            parse(&src[..end]);
        }
        for junk in [
            "<", "</", "<a", "<a b", "<a b=", "<a b='", "&", "&#", "&#x;", "<!--", "<a =>", "</ >",
        ] {
            parse(junk);
        }
    }
}
