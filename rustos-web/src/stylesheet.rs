//! A deliberately small CSS subset: `<style>` blocks are parsed into rules with
//! simple compound selectors (`tag`, `.class`, `#id`, combinations) and applied
//! in specificity order (tag < class < id).
//!
//! Documented limitations: no descendant/child/sibling combinators (the last
//! compound selector is used as the subject), no `@media` evaluation (the block
//! is skipped), no pseudo-classes (they are stripped), no `!important`, no
//! shorthand expansion beyond `margin`/`padding`.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::html::Element;
use crate::style::Style;

/// Upper bound on rules kept per document (protects the 16 MiB heap).
pub const MAX_RULES: usize = 256;

/// One compound selector, e.g. `div.notice#main`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Selector {
    /// Required tag name.
    pub tag: Option<String>,
    /// Required id.
    pub id: Option<String>,
    /// Required classes (all of them).
    pub classes: Vec<String>,
}

impl Selector {
    /// Parse a single compound selector; `None` for the empty selector.
    pub fn parse(input: &str) -> Option<Self> {
        // Combinators are not supported: the right-most compound is the subject.
        let subject = input
            .rsplit(['>', '+', '~'])
            .find(|part| !part.trim().is_empty())?;
        // Drop pseudo-classes/pseudo-elements (`a:hover`, `p::first-line`).
        let subject = subject.split(':').next().unwrap_or("");
        // Drop attribute selectors (`[href]`, `[type="x"]`).
        let subject = strip_attribute_selectors(subject);

        let mut selector = Selector::default();
        let mut current = String::new();
        let mut mode = 0u8; // 0 = tag, 1 = class, 2 = id

        let flush = |selector: &mut Selector, current: &mut String, mode: u8| {
            if current.is_empty() {
                return;
            }
            match mode {
                1 => selector.classes.push(current.to_ascii_lowercase()),
                2 => selector.id = Some(current.clone()),
                _ => selector.tag = Some(current.to_ascii_lowercase()),
            }
            current.clear();
        };

        for character in subject.chars() {
            match character {
                '.' => {
                    flush(&mut selector, &mut current, mode);
                    mode = 1;
                }
                '#' => {
                    flush(&mut selector, &mut current, mode);
                    mode = 2;
                }
                '*' | ' ' | '\t' | '\n' | '\r' => {
                    flush(&mut selector, &mut current, mode);
                    mode = 0;
                }
                other => current.push(other),
            }
        }
        flush(&mut selector, &mut current, mode);

        if selector.tag.is_none() && selector.id.is_none() && selector.classes.is_empty() {
            // `*` matches everything; anything else that produced no parts is
            // not a usable selector.
            if subject.contains('*') {
                Some(Selector::default())
            } else {
                None
            }
        } else {
            Some(selector)
        }
    }

    /// 1 for tag, 2 for class, 3 for id, added up (a small specificity model).
    pub fn specificity(&self) -> u8 {
        let tag = u8::from(self.tag.is_some());
        let id = if self.id.is_some() { 3 } else { 0 };
        let classes = self.classes.len().min(3) as u8 * 2;
        tag + id + classes
    }

    /// Does this selector match `element`?
    pub fn matches(&self, element: &Element) -> bool {
        if let Some(tag) = &self.tag {
            if !element.tag.eq_ignore_ascii_case(tag) {
                return false;
            }
        }
        if let Some(id) = &self.id {
            if element.id() != Some(id.as_str()) {
                return false;
            }
        }
        if !self.classes.is_empty() {
            let present: Vec<&str> = element.classes().collect();
            for class in &self.classes {
                if !present.iter().any(|item| item.eq_ignore_ascii_case(class)) {
                    return false;
                }
            }
        }
        true
    }
}

fn strip_attribute_selectors(input: &str) -> &str {
    // Cheap approach: if there is no '[', nothing to do. Otherwise copy into a
    // leaked-free scratch is impossible with &str output, so callers that need
    // attribute selectors must pre-strip. We simply cut everything after '['.
    match input.find('[') {
        Some(index) => input.get(..index).unwrap_or(""),
        None => input,
    }
}

/// One style rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// The selector that must match.
    pub selector: Selector,
    /// Raw declaration text applied when it does.
    pub declarations: String,
}

/// A parsed stylesheet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StyleSheet {
    /// Rules in source order.
    pub rules: Vec<Rule>,
}

impl StyleSheet {
    /// Parse one or more `<style>` blocks (order is preserved).
    pub fn parse_many(blocks: &[String]) -> Self {
        let mut sheet = StyleSheet::default();
        for block in blocks {
            sheet.parse_into(block);
            if sheet.rules.len() >= MAX_RULES {
                break;
            }
        }
        sheet
    }

    /// Parse a single stylesheet (appending to whatever is already there).
    pub fn parse_into(&mut self, css: &str) {
        let css = strip_comments(css);
        let bytes = css.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            while index < bytes.len()
                && (bytes[index].is_ascii_whitespace() || bytes[index] == b';')
            {
                index += 1;
            }
            if index >= bytes.len() {
                break;
            }
            if bytes[index] == b'@' {
                // Skip at-rules wholesale (@media, @import, @font-face, ...).
                index = skip_at_rule(&css, index);
                continue;
            }
            let selector_end = match css.get(index..).and_then(|rest| rest.find('{')) {
                Some(offset) => index + offset,
                None => break,
            };
            let block_end = match css.get(selector_end..).and_then(|rest| rest.find('}')) {
                Some(offset) => selector_end + offset,
                None => css.len(),
            };
            let selector_text = css.get(index..selector_end).unwrap_or("");
            let declarations = css
                .get(selector_end + 1..block_end)
                .unwrap_or("")
                .trim()
                .to_string();
            if !declarations.is_empty() {
                for part in selector_text.split(',') {
                    if let Some(selector) = Selector::parse(part) {
                        self.rules.push(Rule {
                            selector,
                            declarations: declarations.clone(),
                        });
                        if self.rules.len() >= MAX_RULES {
                            return;
                        }
                    }
                }
            }
            index = block_end + 1;
        }
    }

    /// Apply every matching rule to `style`, lowest specificity first.
    pub fn apply(&self, element: &Element, style: &mut Style) {
        for pass in 1..=3u8 {
            for rule in &self.rules {
                let specificity = rule.selector.specificity();
                let bucket = match specificity {
                    0 | 1 => 1,
                    2 | 3 => 2,
                    _ => 3,
                };
                if bucket != pass {
                    continue;
                }
                if rule.selector.matches(element) {
                    style.apply_declarations(&rule.declarations);
                }
            }
        }
    }

    /// Number of rules (used by tests and the browser's about page).
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// True when no rule was parsed.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(rest.get(..start).unwrap_or(""));
        match rest.get(start + 2..).and_then(|tail| tail.find("*/")) {
            Some(offset) => rest = rest.get(start + 2 + offset + 2..).unwrap_or(""),
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn skip_at_rule(css: &str, index: usize) -> usize {
    let rest = css.get(index..).unwrap_or("");
    let brace = rest.find('{');
    let semicolon = rest.find(';');
    match (brace, semicolon) {
        (Some(brace), Some(semicolon)) if semicolon < brace => index + semicolon + 1,
        (Some(brace), _) => {
            // Skip a balanced block (nested braces are allowed in @media).
            let mut depth = 0usize;
            let mut cursor = index + brace;
            let bytes = css.as_bytes();
            while cursor < bytes.len() {
                match bytes[cursor] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return cursor + 1;
                        }
                    }
                    _ => {}
                }
                cursor += 1;
            }
            css.len()
        }
        (None, Some(semicolon)) => index + semicolon + 1,
        (None, None) => css.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn element(tag: &str, attrs: &[(&str, &str)]) -> Element {
        Element {
            tag: tag.to_string(),
            attrs: attrs
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        }
    }

    #[test]
    fn parses_compound_selectors() {
        let selector = Selector::parse("div.notice#main").expect("selector");
        assert_eq!(selector.tag.as_deref(), Some("div"));
        assert_eq!(selector.id.as_deref(), Some("main"));
        assert_eq!(selector.classes, vec!["notice".to_string()]);
    }

    #[test]
    fn handles_combinators_and_pseudos() {
        let descendant = Selector::parse("nav ul li").expect("selector");
        assert_eq!(descendant.tag.as_deref(), Some("li"));
        let pseudo = Selector::parse("a:hover").expect("selector");
        assert_eq!(pseudo.tag.as_deref(), Some("a"));
        assert!(pseudo.matches(&element("a", &[])));
        let wildcard = Selector::parse("*").expect("selector");
        assert!(wildcard.matches(&element("anything", &[])));
        assert_eq!(Selector::parse("   "), None);
    }

    #[test]
    fn matches_classes_and_ids() {
        let target = element("p", &[("class", "a b"), ("id", "x")]);
        assert!(Selector::parse(".a").unwrap().matches(&target));
        assert!(Selector::parse(".b.a").unwrap().matches(&target));
        assert!(!Selector::parse(".c").unwrap().matches(&target));
        assert!(Selector::parse("#x").unwrap().matches(&target));
        assert!(!Selector::parse("#y").unwrap().matches(&target));
        assert!(Selector::parse("p.a#x").unwrap().matches(&target));
        assert!(!Selector::parse("div").unwrap().matches(&target));
    }

    #[test]
    fn specificity_orders_ids_above_classes() {
        let tag = Selector::parse("p").unwrap().specificity();
        let class = Selector::parse(".a").unwrap().specificity();
        let id = Selector::parse("#x").unwrap().specificity();
        assert!(tag < class && class < id);
    }

    #[test]
    fn applies_stylesheet_in_specificity_order() {
        let sheet = StyleSheet::parse_many(&[String::from(
            "p { color: #ff0000 } .note { color: #00ff00 } #x { color: #0000ff }",
        )]);
        assert_eq!(sheet.len(), 3);

        let mut plain = Style::for_tag("p");
        sheet.apply(&element("p", &[]), &mut plain);
        assert_eq!(plain.color, Some(crate::color::Color::rgb(255, 0, 0)));

        let mut with_class = Style::for_tag("p");
        sheet.apply(&element("p", &[("class", "note")]), &mut with_class);
        assert_eq!(with_class.color, Some(crate::color::Color::rgb(0, 255, 0)));

        let mut with_id = Style::for_tag("p");
        sheet.apply(&element("p", &[("id", "x")]), &mut with_id);
        assert_eq!(with_id.color, Some(crate::color::Color::rgb(0, 0, 255)));
    }

    #[test]
    fn skips_at_rules_and_comments() {
        let css = String::from(
            "@media (max-width: 10px) { p { color: red } } /* c */ body { color: #123456 } @import url(x);",
        );
        let sheet = StyleSheet::parse_many(&[css]);
        assert_eq!(sheet.len(), 1);
        assert!(sheet.rules[0].declarations.contains("#123456"));
    }

    #[test]
    fn survives_malformed_css() {
        for css in [
            "",
            "{",
            "}",
            "p{",
            "p{color}",
            "@media",
            ";;;",
            ".a{color:}",
            "p{color:#zz}",
        ] {
            let sheet = StyleSheet::parse_many(&[String::from(css)]);
            let mut style = Style::default();
            sheet.apply(&element("p", &[("class", "a")]), &mut style);
        }
    }

    #[test]
    fn rule_budget_is_enforced() {
        let mut css = String::new();
        for index in 0..(MAX_RULES * 2) {
            css.push_str(&alloc::format!(".c{index}{{color:red}}"));
        }
        let sheet = StyleSheet::parse_many(&[css]);
        assert!(sheet.len() <= MAX_RULES);
    }
}
