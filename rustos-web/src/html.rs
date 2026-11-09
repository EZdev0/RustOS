//! Forgiving HTML tokenizer and tree builder.
//!
//! This is *not* html5ever: it implements the subset that real 2000s-era web
//! pages (and the modern "readable" web) need, with three hard rules:
//!
//! 1. **Never panic.** Real pages contain unclosed tags, stray `<`, broken
//!    entities and megabytes of script. Every index is bounds-checked and the
//!    node count plus nesting depth are capped.
//! 2. **No recursion while parsing** (an explicit stack of open elements), so a
//!    page like `<div><div><div>…` cannot blow the 8 KiB kernel stack.
//! 3. **Cheap.** One pass over the input, one allocation per node.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Maximum element nesting depth that is materialised in the tree.
pub const MAX_DEPTH: usize = 64;
/// Maximum number of nodes in a document.
pub const MAX_NODES: usize = 20_000;

/// Elements that never have children or an end tag.
const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Elements whose raw text content must not be parsed as markup.
const RAW_TEXT_ELEMENTS: &[&str] = &["script", "style", "textarea", "title", "noscript", "xmp"];

/// Elements that implicitly close a previous element with the same tag.
const AUTO_CLOSING: &[&str] = &[
    "p", "li", "dt", "dd", "tr", "td", "th", "option", "thead", "tbody",
];

/// An element: tag name (lower-case) plus attributes (lower-case names).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Element {
    /// Lower-case tag name.
    pub tag: String,
    /// Attribute `(name, raw value)` pairs in source order.
    pub attrs: Vec<(String, String)>,
}

impl Element {
    /// Look up an attribute value (already lower-case names).
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The `class` attribute split into tokens.
    pub fn classes(&self) -> impl Iterator<Item = &str> {
        self.attr("class").unwrap_or("").split_ascii_whitespace()
    }

    /// The `id` attribute, if present and non-empty.
    pub fn id(&self) -> Option<&str> {
        self.attr("id").filter(|id| !id.is_empty())
    }
}

/// What a [`Node`] is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum NodeKind {
    /// The synthetic root.
    #[default]
    Document,
    /// An element with children.
    Element(Element),
    /// Collapsible text.
    Text(String),
}

/// A DOM node. Children are stored in source order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Node {
    /// Node payload.
    pub kind: NodeKind,
    /// Child nodes.
    pub children: Vec<Node>,
}

impl Node {
    /// Create a document root node.
    pub fn document() -> Self {
        Self {
            kind: NodeKind::Document,
            children: Vec::new(),
        }
    }

    /// Create an element node with no children.
    pub fn new_element(tag: &str, attrs: Vec<(String, String)>) -> Self {
        Self {
            kind: NodeKind::Element(Element {
                tag: tag.to_ascii_lowercase(),
                attrs,
            }),
            children: Vec::new(),
        }
    }

    /// Create a text node.
    pub fn text(content: &str) -> Self {
        Self {
            kind: NodeKind::Text(content.to_string()),
            children: Vec::new(),
        }
    }

    /// Tag name for elements.
    pub fn tag(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Element(element) => Some(&element.tag),
            _ => None,
        }
    }

    /// The element payload, for element nodes.
    pub fn as_element(&self) -> Option<&Element> {
        match &self.kind {
            NodeKind::Element(element) => Some(element),
            _ => None,
        }
    }

    /// Attribute lookup for element nodes.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.as_element().and_then(|element| element.attr(name))
    }

    /// Concatenated text of this subtree (iterative, depth-safe).
    pub fn text_content(&self) -> String {
        let mut out = String::new();
        let mut stack: Vec<&Node> = alloc::vec![self];
        while let Some(node) = stack.pop() {
            match &node.kind {
                NodeKind::Text(text) => out.push_str(text),
                _ => {
                    for child in node.children.iter().rev() {
                        stack.push(child);
                    }
                }
            }
        }
        out
    }
}

/// A parsed page.
#[derive(Debug, Clone, Default)]
pub struct Document {
    /// Root of the tree.
    pub root: Node,
    /// Contents of the first `<title>` element (trimmed).
    pub title: Option<String>,
    /// Raw CSS text of every `<style>` block, in document order.
    pub stylesheets: Vec<String>,
    /// Set when the node budget was exhausted (page clipped, no panic).
    pub truncated: bool,
}

impl Document {
    /// Parse `html` into a tree. Never fails; malformed input is repaired.
    pub fn parse(html: &str) -> Self {
        let mut builder = Builder::new();
        builder.run(html);
        builder.finish()
    }
}

struct Open {
    tag: String,
    element: Element,
    children: Vec<Node>,
    /// Index into `stylesheets`/`title` capture only for special tags.
    raw: Option<RawCapture>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RawCapture {
    Style,
    Title,
    Ignore,
}

struct Builder {
    stack: Vec<Open>,
    nodes: usize,
    truncated: bool,
    title: Option<String>,
    stylesheets: Vec<String>,
}

impl Builder {
    fn new() -> Self {
        Self {
            stack: alloc::vec![Open {
                tag: "#document".to_string(),
                element: Element::default(),
                children: Vec::new(),
                raw: None,
            }],
            nodes: 0,
            truncated: false,
            title: None,
            stylesheets: Vec::new(),
        }
    }

    fn finish(mut self) -> Document {
        while self.stack.len() > 1 {
            self.close_top();
        }
        let root = match self.stack.pop() {
            Some(open) => Node {
                kind: NodeKind::Document,
                children: open.children,
            },
            None => Node::document(),
        };
        Document {
            root,
            title: self.title,
            stylesheets: self.stylesheets,
            truncated: self.truncated,
        }
    }

    fn close_top(&mut self) {
        let open = match self.stack.pop() {
            Some(open) => open,
            None => return,
        };
        if self.stack.is_empty() {
            return;
        }
        let node = Node {
            kind: NodeKind::Element(open.element),
            children: open.children,
        };
        self.push_node(node);
    }

    fn push_node(&mut self, node: Node) {
        if let Some(top) = self.stack.last_mut() {
            if self.nodes < MAX_NODES {
                self.nodes += 1;
                top.children.push(node);
            } else {
                self.truncated = true;
            }
        }
    }

    fn close_until(&mut self, tag: &str) {
        // Find the nearest open element with this tag; malformed end tags are
        // ignored, stray ones close nothing.
        let mut target: Option<usize> = None;
        for (index, open) in self.stack.iter().enumerate().rev() {
            if open.tag == tag {
                target = Some(index);
                break;
            }
        }
        if let Some(index) = target {
            while self.stack.len() > index {
                self.close_top();
            }
        }
    }

    fn run(&mut self, html: &str) {
        let bytes = html.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'<' {
                match bytes.get(index + 1).copied() {
                    Some(b'!') => {
                        index = self.skip_doctype_or_comment(html, index);
                    }
                    Some(b'?') => {
                        index = skip_to(html, index, '>');
                    }
                    Some(b'/') => {
                        let (name, next) = read_tag_name(html, index + 2);
                        let tag = name.to_ascii_lowercase();
                        // Skip whatever follows the name (spaces, newlines)
                        // up to and including the closing bracket.
                        index = skip_to(html, next, '>');
                        if !tag.is_empty() {
                            self.close_until(&tag);
                        }
                    }
                    Some(next) if next.is_ascii_alphabetic() => {
                        let (raw_name, mut cursor) = read_tag_name(html, index + 1);
                        let tag = raw_name.to_ascii_lowercase();
                        let (attrs, self_closing, cursor_after) = read_attributes(html, cursor);
                        cursor = cursor_after;
                        index = cursor;
                        if tag.is_empty() {
                            self.push_text("<");
                            continue;
                        }
                        self.start_tag(&tag, attrs, self_closing);
                        if RAW_TEXT_ELEMENTS.contains(&tag.as_str()) {
                            index = self.consume_raw_text(html, index, &tag);
                        }
                    }
                    _ => {
                        self.push_text("<");
                        index += 1;
                    }
                }
            } else {
                let next = match html[index..].find('<') {
                    Some(offset) => index + offset,
                    None => bytes.len(),
                };
                let slice = html.get(index..next).unwrap_or("");
                self.push_text(&decode_entities(slice));
                index = next;
            }
        }
    }

    fn skip_doctype_or_comment(&mut self, html: &str, index: usize) -> usize {
        if html
            .get(index..)
            .map(|rest| rest.starts_with("<!--"))
            .unwrap_or(false)
        {
            return match html.get(index + 4..).and_then(|rest| rest.find("-->")) {
                Some(offset) => index + 4 + offset + 3,
                None => html.len(),
            };
        }
        skip_to(html, index, '>')
    }

    fn consume_raw_text(&mut self, html: &str, index: usize, tag: &str) -> usize {
        // The element was pushed by `start_tag`; it has to be popped again or
        // everything that follows would become its (display:none) child.
        let closing = alloc::format!("</{}", tag);
        let rest = html.get(index..).unwrap_or("");
        let lower = rest.to_ascii_lowercase();
        match lower.find(&closing) {
            Some(offset) => {
                let text = html.get(index..index + offset).unwrap_or("");
                self.capture_raw(tag, text);
                self.close_until(tag);
                // Skip past the closing tag.
                skip_to(html, index + offset, '>')
            }
            None => {
                let text = html.get(index..).unwrap_or("");
                self.capture_raw(tag, text);
                self.close_until(tag);
                html.len()
            }
        }
    }

    fn capture_raw(&mut self, tag: &str, text: &str) {
        match tag {
            "style" => {
                if self.stylesheets.len() < 16 {
                    self.stylesheets.push(text.to_string());
                }
            }
            "title" => {
                let trimmed = text.trim();
                if self.title.is_none() && !trimmed.is_empty() {
                    self.title = Some(decode_entities(trimmed));
                }
            }
            _ => {}
        }
        // Keep `<title>`/`<style>` text out of the rendered tree.
        self.push_node(Node::text(""));
    }

    fn start_tag(&mut self, tag: &str, attrs: Vec<(String, String)>, self_closing: bool) {
        let is_void = VOID_ELEMENTS.contains(&tag) || self_closing;
        if AUTO_CLOSING.contains(&tag) {
            // `<p>` closes a previous `<p>`, `<li>` a previous `<li>`, ...
            let mut depth_guard = 0;
            while self
                .stack
                .last()
                .map(|open| open.tag == tag)
                .unwrap_or(false)
                && depth_guard < MAX_DEPTH
            {
                self.close_top();
                depth_guard += 1;
            }
        }
        let element = Element {
            tag: tag.to_string(),
            attrs,
        };
        if is_void {
            self.push_node(Node::new_element(tag, element.attrs));
            return;
        }
        if self.stack.len() >= MAX_DEPTH {
            // Too deep: materialise as a leaf so the content is not lost.
            self.truncated = true;
            self.push_node(Node::new_element(tag, element.attrs));
            return;
        }
        let raw = match tag {
            "style" => Some(RawCapture::Style),
            "title" => Some(RawCapture::Title),
            "script" | "noscript" => Some(RawCapture::Ignore),
            _ => None,
        };
        self.stack.push(Open {
            tag: tag.to_string(),
            element,
            children: Vec::new(),
            raw,
        });
    }

    fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if text.trim().is_empty() {
            // Inter-element whitespace is meaningless for block layout and
            // would otherwise dominate the node budget of real pages. Inside
            // pre-formatted containers it is significant, so keep it there.
            let preserve = self
                .stack
                .last()
                .map(|open| matches!(open.tag.as_str(), "pre" | "textarea" | "code" | "xmp"))
                .unwrap_or(false);
            if !preserve {
                return;
            }
        }
        if let Some(top) = self.stack.last_mut() {
            if matches!(top.raw, Some(RawCapture::Ignore)) {
                return;
            }
            if let Some(last) = top.children.last_mut() {
                if let NodeKind::Text(existing) = &mut last.kind {
                    existing.push_str(text);
                    return;
                }
            }
        }
        self.push_node(Node::text(text));
    }
}

/// Advance past the next `>` (or to the end of input).
fn skip_to(html: &str, index: usize, terminator: char) -> usize {
    match html.get(index..).and_then(|rest| rest.find(terminator)) {
        Some(offset) => index + offset + 1,
        None => html.len(),
    }
}

/// Read a tag name starting at `index`; returns `(name, index_after_name)`.
fn read_tag_name(html: &str, index: usize) -> (&str, usize) {
    let bytes = html.as_bytes();
    let start = index.min(bytes.len());
    let mut end = start;
    while end < bytes.len() {
        let byte = bytes[end];
        if byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' || byte == b':' {
            end += 1;
        } else {
            break;
        }
    }
    let name = html.get(start..end).unwrap_or("");
    (name, end)
}

/// Parse attributes up to and including the closing `>`.
fn read_attributes(html: &str, index: usize) -> (Vec<(String, String)>, bool, usize) {
    let bytes = html.as_bytes();
    let mut cursor = index.min(bytes.len());
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut self_closing = false;

    loop {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        match bytes.get(cursor).copied() {
            None => break,
            Some(b'>') => {
                cursor += 1;
                break;
            }
            Some(b'/') => {
                self_closing = true;
                cursor += 1;
                continue;
            }
            _ => {}
        }

        let name_start = cursor;
        while cursor < bytes.len() {
            let byte = bytes[cursor];
            if byte.is_ascii_whitespace() || byte == b'=' || byte == b'>' || byte == b'/' {
                break;
            }
            cursor += 1;
        }
        let name = html
            .get(name_start..cursor)
            .unwrap_or("")
            .to_ascii_lowercase();

        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }

        let mut value = String::new();
        if bytes.get(cursor) == Some(&b'=') {
            cursor += 1;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            match bytes.get(cursor).copied() {
                Some(quote) if quote == b'"' || quote == b'\'' => {
                    cursor += 1;
                    let value_start = cursor;
                    while cursor < bytes.len() && bytes[cursor] != quote {
                        cursor += 1;
                    }
                    value = decode_entities(html.get(value_start..cursor).unwrap_or(""));
                    if cursor < bytes.len() {
                        cursor += 1;
                    }
                }
                _ => {
                    let value_start = cursor;
                    while cursor < bytes.len() {
                        let byte = bytes[cursor];
                        if byte.is_ascii_whitespace() || byte == b'>' {
                            break;
                        }
                        cursor += 1;
                    }
                    value = decode_entities(html.get(value_start..cursor).unwrap_or(""));
                }
            }
        }

        if !name.is_empty() && attrs.len() < 32 {
            let duplicate = attrs.iter().any(|(key, _)| *key == name);
            if !duplicate {
                attrs.push((name, value));
            }
        }
    }

    (attrs, self_closing, cursor)
}

/// Decode the handful of HTML entities that appear on real pages.
///
/// Typographic characters are folded to their ASCII equivalents because the
/// kernel renderer only has an 8x8 ASCII font — `&mdash;` must not turn into a
/// blank box on screen.
pub fn decode_entities(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(rest.get(..amp).unwrap_or(""));
        let after = rest.get(amp..).unwrap_or("");
        let candidate = after.get(1..).unwrap_or("");
        let mut consumed = 1;
        let mut replacement: Option<&str> = None;
        if let Some(semi) = candidate.find(';').filter(|&semi| semi <= 10) {
            let name = candidate.get(..semi).unwrap_or("");
            if let Some(text) = named_entity(name) {
                replacement = Some(text);
                consumed = semi + 2;
            } else if let Some(text) = numeric_entity(name) {
                out.push_str(&text);
                consumed = semi + 2;
            }
        }
        if let Some(text) = replacement {
            out.push_str(text);
        } else if consumed == 1 {
            out.push('&');
        }
        rest = rest.get(amp + consumed..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

fn named_entity(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => " ",
        "ensp" | "emsp" | "thinsp" => " ",
        "mdash" => "-",
        "ndash" => "-",
        "hellip" => "...",
        "bull" => "*",
        "middot" => ".",
        "times" => "x",
        "divide" => "/",
        "plusmn" => "+-",
        "deg" => "deg",
        "copy" => "(c)",
        "reg" => "(R)",
        "trade" => "(TM)",
        "euro" => "EUR",
        "pound" => "GBP",
        "yen" => "JPY",
        "cent" => "c",
        "sect" => "S",
        "para" => "P",
        "laquo" => "<<",
        "raquo" => ">>",
        "ldquo" => "\"",
        "rdquo" => "\"",
        "lsquo" => "'",
        "rsquo" => "'",
        "shy" => "",
        "zwnj" | "zwj" | "lrm" | "rlm" => "",
        "frac12" => "1/2",
        "frac14" => "1/4",
        "frac34" => "3/4",
        "micro" => "u",
        "prime" => "'",
        "Prime" => "\"",
        _ => return None,
    })
}

fn numeric_entity(name: &str) -> Option<String> {
    let value = name.strip_prefix('#')?;
    let code = if let Some(hex) = value.strip_prefix(['x', 'X']) {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        value.parse::<u32>().ok()?
    };
    let character = char::from_u32(code)?;
    let folded = match character {
        '\u{00A0}' => ' ',
        '\u{2010}'..='\u{2015}' => '-',
        '\u{2018}' | '\u{2019}' => '\'',
        '\u{201C}' | '\u{201D}' => '"',
        '\u{2022}' => '*',
        '\u{2026}' => return Some("...".to_string()),
        other => other,
    };
    Some(folded.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_first<'a>(node: &'a Node, tag: &str) -> Option<&'a Node> {
        if node.tag() == Some(tag) {
            return Some(node);
        }
        for child in &node.children {
            if let Some(found) = find_first(child, tag) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn parses_nested_elements() {
        let document =
            Document::parse("<html><body><h1>Hi</h1><p>Hello <b>world</b></p></body></html>");
        let body = find_first(&document.root, "body").expect("body");
        assert_eq!(body.children.len(), 2);
        assert_eq!(body.children[0].tag(), Some("h1"));
        assert_eq!(body.children[0].text_content(), "Hi");
        let paragraph = &body.children[1];
        assert_eq!(paragraph.text_content(), "Hello world");
        assert_eq!(paragraph.children.len(), 2);
    }

    #[test]
    fn repairs_unclosed_tags() {
        let document = Document::parse("<div><p>one<p>two</div><span>three");
        let text = document.root.text_content();
        assert!(text.contains("one"));
        assert!(text.contains("two"));
        assert!(text.contains("three"));
    }

    #[test]
    fn captures_title_and_stylesheets() {
        let document = Document::parse(
            "<html><head><title> A &amp; B </title><style>body { color: red }</style></head><body>x</body></html>",
        );
        assert_eq!(document.title.as_deref(), Some("A & B"));
        assert_eq!(document.stylesheets.len(), 1);
        assert!(document.stylesheets[0].contains("color: red"));
        assert!(!document.root.text_content().contains("color"));
    }

    #[test]
    fn strips_comments_and_scripts() {
        let document =
            Document::parse("<!-- hidden --><p>visible</p><script>var x = '<p>nope</p>';</script>");
        let text = document.root.text_content();
        assert!(!text.contains("hidden"));
        assert!(!text.contains("nope"));
        assert!(text.contains("visible"));
    }

    #[test]
    fn parses_attributes_and_voids() {
        let document = Document::parse(
            r#"<a href="/x" class="one two" data-x=1>link</a><br><img src="i.png" alt="An image">"#,
        );
        let anchor = find_first(&document.root, "a").expect("a");
        assert_eq!(anchor.attr("href"), Some("/x"));
        let element = anchor.as_element().expect("element");
        let classes: Vec<&str> = element.classes().collect();
        assert_eq!(classes, alloc::vec!["one", "two"]);
        let image = find_first(&document.root, "img").expect("img");
        assert_eq!(image.attr("alt"), Some("An image"));
        assert!(image.children.is_empty());
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(decode_entities("a &amp; b"), "a & b");
        assert_eq!(decode_entities("&lt;tag&gt;"), "<tag>");
        assert_eq!(decode_entities("caf&eacute;"), "caf&eacute;");
        assert_eq!(decode_entities("&#65;&#x42;"), "AB");
        assert_eq!(decode_entities("100&nbsp;%"), "100 %");
        assert_eq!(decode_entities("x &unknown; y"), "x &unknown; y");
        assert_eq!(decode_entities("&mdash;"), "-");
        assert_eq!(decode_entities("AT&T"), "AT&T");
        assert_eq!(decode_entities(""), "");
    }

    #[test]
    fn survives_hostile_markup() {
        let inputs = [
            "<",
            "<<<<>>>>",
            "<p",
            "<a href=",
            "<a href='unterminated",
            "<!doctype html",
            "<!--",
            "</>",
            "<style>",
            "&#999999999;",
            "&#xZZ;",
            "<div><div><div>",
        ];
        for input in inputs {
            let document = Document::parse(input);
            let _ = document.root.text_content();
        }
    }

    #[test]
    fn deep_nesting_is_bounded() {
        let mut html = String::new();
        for _ in 0..500 {
            html.push_str("<div>");
        }
        html.push_str("deep");
        let document = Document::parse(&html);
        assert!(document.root.text_content().contains("deep"));
        assert!(document.truncated || !document.root.children.is_empty());
    }

    #[test]
    fn many_nodes_are_bounded() {
        let mut html = String::new();
        for _ in 0..(MAX_NODES + 500) {
            html.push_str("<i>x</i>");
        }
        let document = Document::parse(&html);
        assert!(document.truncated);
    }

    #[test]
    fn utf8_text_survives() {
        let document = Document::parse("<p>Grüße, 世界</p>");
        assert_eq!(document.root.text_content(), "Grüße, 世界");
    }
}
