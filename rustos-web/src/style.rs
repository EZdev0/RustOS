//! A tiny style model: the built-in UA stylesheet plus a declaration parser
//! that understands the handful of CSS properties this engine can paint.
//!
//! Everything is integer-only and `Copy`, so a style can be cheaply cloned
//! down the tree during layout (no reference counting, no borrow puzzles in a
//! `no_std` kernel).

use crate::color::Color;

/// Outer display type of a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    /// Takes the full available width and stacks vertically.
    Block,
    /// Flows inside the current line.
    Inline,
    /// Not rendered at all (`display: none`, `<head>` contents, ...).
    None,
}

/// Horizontal alignment of text inside a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// Left aligned (default).
    Left,
    /// Centred.
    Center,
    /// Right aligned.
    Right,
}

/// List marker style for `li` elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    /// No marker.
    None,
    /// A bullet, rendered as `*`.
    Bullet,
    /// An ordered marker, rendered as `1.` at layout time.
    Number,
}

/// Computed style of one element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    /// Outer display type.
    pub display: Display,
    /// Glyph scale (1 = 8x8 px, 2 = 16x16 px, ...).
    pub font_scale: u8,
    /// Request a faux-bold double strike.
    pub bold: bool,
    /// Text colour; `None` inherits from the parent box.
    pub color: Option<Color>,
    /// Background fill behind the box.
    pub background: Option<Color>,
    /// Text alignment.
    pub align: Align,
    /// Horizontal padding applied to the whole box.
    pub indent: u16,
    /// Vertical space above the box.
    pub margin_before: u16,
    /// Vertical space below the box.
    pub margin_after: u16,
    /// Draw an underline below every glyph run.
    pub underline: bool,
    /// Preserve whitespace and never wrap lines.
    pub preformatted: bool,
    /// Draw a 1 px frame around the box (tables, inputs, images).
    pub border: bool,
    /// List marker style.
    pub marker: Marker,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            display: Display::Block,
            font_scale: 1,
            bold: false,
            color: None,
            background: None,
            align: Align::Left,
            indent: 0,
            margin_before: 0,
            margin_after: 0,
            underline: false,
            preformatted: false,
            border: false,
            marker: Marker::None,
        }
    }
}

impl Style {
    /// Inherit the inheritable properties from a parent style.
    pub fn inherit(parent: &Style) -> Self {
        Self {
            color: parent.color,
            font_scale: parent.font_scale,
            bold: parent.bold,
            underline: parent.underline,
            align: parent.align,
            ..Style::default()
        }
    }

    /// The built-in stylesheet, keyed by tag name.
    pub fn for_tag(tag: &str) -> Self {
        let mut style = Style::default();
        match tag {
            "html" | "body" => {}
            "title" | "head" | "meta" | "link" | "style" | "script" | "noscript" | "base"
            | "template" | "thead" | "colgroup" | "col" => {
                style.display = Display::None;
            }
            "h1" => {
                style.font_scale = 3;
                style.bold = true;
                style.margin_before = 12;
                style.margin_after = 8;
            }
            "h2" => {
                style.font_scale = 2;
                style.bold = true;
                style.margin_before = 10;
                style.margin_after = 6;
            }
            "h3" | "h4" | "h5" | "h6" => {
                style.font_scale = 2;
                style.bold = true;
                style.margin_before = 8;
                style.margin_after = 4;
            }
            "p" => {
                style.margin_before = 4;
                style.margin_after = 8;
            }
            "div" | "section" | "article" | "header" | "footer" | "nav" | "main" | "aside"
            | "figure" | "figcaption" | "form" | "fieldset" | "address" | "details" | "summary" => {
            }
            "ul" | "ol" => {
                style.margin_before = 2;
                style.margin_after = 6;
                style.indent = 24;
            }
            "li" => {
                style.margin_before = 1;
                style.margin_after = 2;
                style.marker = Marker::Bullet;
            }
            "blockquote" => {
                style.indent = 24;
                style.margin_before = 4;
                style.margin_after = 6;
                style.border = true;
            }
            "pre" => {
                style.preformatted = true;
                style.margin_before = 4;
                style.margin_after = 6;
                style.indent = 8;
                style.background = Some(Color::rgb(238, 240, 244));
            }
            "code" | "kbd" | "samp" | "tt" => {
                style.display = Display::Inline;
                style.preformatted = true;
                style.background = Some(Color::rgb(240, 240, 240));
            }
            "hr" => {
                style.margin_before = 8;
                style.margin_after = 8;
            }
            "a" => {
                style.display = Display::Inline;
                style.color = Some(Color::rgb(0, 64, 200));
                style.underline = true;
            }
            "b" | "strong" | "dt" => {
                style.display = Display::Inline;
                style.bold = true;
            }
            "i" | "em" | "cite" | "var" | "dfn" => {
                style.display = Display::Inline;
            }
            "u" => {
                style.display = Display::Inline;
                style.underline = true;
            }
            "span" | "font" | "small" | "big" | "sub" | "sup" | "label" | "abbr" | "time"
            | "mark" | "q" | "s" | "del" | "ins" => {
                style.display = Display::Inline;
            }
            "center" => {
                style.align = Align::Center;
            }
            "table" => {
                style.margin_before = 6;
                style.margin_after = 6;
                style.border = true;
            }
            "tr" | "tbody" | "tfoot" => {}
            "td" => {
                style.indent = 6;
                style.margin_before = 1;
                style.margin_after = 1;
                style.border = true;
                style.background = Some(Color::rgb(248, 248, 250));
            }
            "th" => {
                style.display = Display::Block;
                style.indent = 6;
                style.margin_before = 1;
                style.margin_after = 1;
                style.border = true;
                style.bold = true;
                style.background = Some(Color::rgb(228, 230, 236));
            }
            "img" => {
                style.margin_before = 4;
                style.margin_after = 4;
            }
            "input" | "button" | "select" | "textarea" => {
                style.display = Display::Inline;
                style.border = true;
                style.background = Some(Color::rgb(236, 236, 240));
                style.indent = 2;
            }
            "br" => {
                style.display = Display::Inline;
            }
            _ => {}
        }
        style
    }

    /// Apply `prop: value; ...` declarations (inline `style=""` or a CSS rule).
    ///
    /// Unknown properties are ignored, malformed declarations are skipped, and
    /// nothing here can panic.
    pub fn apply_declarations(&mut self, declarations: &str) {
        for declaration in declarations.split(';') {
            let (property, value) = match declaration.split_once(':') {
                Some(parts) => parts,
                None => continue,
            };
            let property = property.trim().to_ascii_lowercase();
            let value = value.trim().trim_end_matches("!important").trim();
            if value.is_empty() {
                continue;
            }
            self.apply_property(&property, value);
        }
    }

    fn apply_property(&mut self, property: &str, value: &str) {
        let lower = value.to_ascii_lowercase();
        match property {
            "display" => {
                self.display = match lower.as_str() {
                    "none" => Display::None,
                    "inline" | "inline-block" | "inline-flex" | "inline-table" => Display::Inline,
                    _ => Display::Block,
                };
            }
            "color" => {
                if let Some(color) = Color::parse(value) {
                    self.color = Some(color);
                }
            }
            "background" | "background-color" => {
                if let Some(color) = Color::parse(value) {
                    self.background = Some(color);
                }
            }
            "font-weight" => match lower.as_str() {
                "bold" | "bolder" | "600" | "700" | "800" | "900" => self.bold = true,
                "normal" | "lighter" | "400" | "300" | "200" | "100" => self.bold = false,
                _ => {}
            },
            "font-size" => {
                if let Some(scale) = scale_from_font_size(&lower) {
                    self.font_scale = scale;
                }
            }
            "text-align" => {
                self.align = match lower.as_str() {
                    "center" | "middle" => Align::Center,
                    "right" | "end" => Align::Right,
                    _ => Align::Left,
                };
            }
            "text-decoration" | "text-decoration-line" => {
                if lower.contains("underline") {
                    self.underline = true;
                } else if lower.contains("none") {
                    self.underline = false;
                }
            }
            "margin" => {
                if let Some(value) = parse_px(&lower) {
                    self.margin_before = value;
                    self.margin_after = value;
                }
            }
            "margin-top" => {
                if let Some(value) = parse_px(&lower) {
                    self.margin_before = value;
                }
            }
            "margin-bottom" => {
                if let Some(value) = parse_px(&lower) {
                    self.margin_after = value;
                }
            }
            "padding" | "padding-left" | "padding-inline-start" => {
                if let Some(value) = parse_px(&lower) {
                    self.indent = value;
                }
            }
            "border" | "border-width" | "border-style" => {
                if !lower.contains("none") && !lower.contains("0") {
                    self.border = true;
                }
            }
            "white-space" => {
                self.preformatted = matches!(lower.as_str(), "pre" | "pre-wrap" | "break-spaces");
            }
            "list-style-type" => {
                self.marker = match lower.as_str() {
                    "none" => Marker::None,
                    "decimal" | "decimal-leading-zero" | "lower-alpha" | "upper-alpha" => {
                        Marker::Number
                    }
                    _ => Marker::Bullet,
                };
            }
            _ => {}
        }
    }
}

/// Map a CSS `font-size` value onto a glyph scale.
fn scale_from_font_size(value: &str) -> Option<u8> {
    match value.trim() {
        "xx-small" => return Some(1),
        "x-small" | "small" => return Some(1),
        "medium" => return Some(1),
        "large" => return Some(2),
        "x-large" | "xx-large" | "xxx-large" => return Some(3),
        "smaller" | "larger" => return Some(2),
        _ => {}
    }
    let pixels = parse_px(value)?;
    let scale = (pixels / 8).clamp(1, 4);
    Some(scale as u8)
}

/// Parse a CSS length into whole pixels (unitless values are pixels).
pub fn parse_px(value: &str) -> Option<u16> {
    let value = value.trim();
    let numeric = value
        .trim_end_matches("px")
        .trim_end_matches("pt")
        .trim_end_matches("em")
        .trim_end_matches('%')
        .trim();
    if numeric.is_empty() {
        return None;
    }
    // Allow "12.5" style values without pulling in float parsing.
    let (whole, fraction) = match numeric.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (numeric, ""),
    };
    if !whole.bytes().all(|b| b.is_ascii_digit()) || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    let whole_value: u32 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    // Fractional pixels are truncated: glyphs are 8 px wide anyway.
    Some(whole_value.min(512) as u16)
}

/// Parse `style`/`class` attributes into a style, starting from `parent`.
pub fn style_for_element(tag: &str, parent: &Style) -> Style {
    let mut style = Style::inherit(parent);
    let ua = Style::for_tag(tag);
    merge_ua(&mut style, &ua);
    style
}

/// Merge the non-inherited properties of a UA rule into an inherited style.
fn merge_ua(style: &mut Style, ua: &Style) {
    style.display = ua.display;
    style.margin_before = ua.margin_before;
    style.margin_after = ua.margin_after;
    style.indent = ua.indent;
    style.background = ua.background;
    style.border = ua.border;
    style.marker = ua.marker;
    style.preformatted = ua.preformatted;
    if ua.font_scale > style.font_scale {
        style.font_scale = ua.font_scale;
    }
    style.bold |= ua.bold;
    style.underline |= ua.underline;
    if ua.color.is_some() {
        style.color = ua.color;
    }
    if ua.align != Align::Left {
        style.align = ua.align;
    }
}

/// Debug helper used by tests: render a style as a compact string.
#[cfg(test)]
pub fn describe(style: &Style) -> String {
    alloc::format!(
        "{:?}/s{}/{}/{}",
        style.display,
        style.font_scale,
        style.margin_before,
        style.margin_after
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ua_sheet_has_sensible_defaults() {
        let h1 = Style::for_tag("h1");
        assert_eq!(h1.display, Display::Block);
        assert_eq!(h1.font_scale, 3);
        assert!(h1.bold);

        assert_eq!(Style::for_tag("script").display, Display::None);
        assert_eq!(Style::for_tag("a").display, Display::Inline);
        assert!(Style::for_tag("a").underline);
        assert!(Style::for_tag("pre").preformatted);
        assert_eq!(Style::for_tag("li").marker, Marker::Bullet);
        assert!(Style::for_tag("td").border);
    }

    #[test]
    fn inline_declarations_override_ua() {
        let mut style = Style::for_tag("p");
        style.apply_declarations("color: #ff0000; text-align: center; margin-top: 12px");
        assert_eq!(style.color, Some(Color::rgb(255, 0, 0)));
        assert_eq!(style.align, Align::Center);
        assert_eq!(style.margin_before, 12);
    }

    #[test]
    fn display_none_wins() {
        let mut style = Style::default();
        style.apply_declarations("display:none");
        assert_eq!(style.display, Display::None);
    }

    #[test]
    fn font_size_maps_to_scale() {
        let mut style = Style::default();
        style.apply_declarations("font-size: 24px");
        assert_eq!(style.font_scale, 3);
        style.apply_declarations("font-size: large");
        assert_eq!(style.font_scale, 2);
        style.apply_declarations("font-size: 1px");
        assert_eq!(style.font_scale, 1);
        style.apply_declarations("font-size: enormous");
        assert_eq!(style.font_scale, 1);
    }

    #[test]
    fn malformed_declarations_are_ignored() {
        let mut style = Style::default();
        style.apply_declarations("");
        style.apply_declarations("garbage");
        style.apply_declarations("color: ; ;; ;; ");
        style.apply_declarations("color: not-a-colour");
        style.apply_declarations("margin: abc");
        style.apply_declarations("font-size: -3px");
        assert_eq!(style.display, Display::Block);
        assert_eq!(style.color, None);
        assert_eq!(style.margin_before, 0);
        assert_eq!(style.font_scale, 1);
    }

    #[test]
    fn ua_merge_inherits_text_properties() {
        let parent = Style {
            color: Some(Color::rgb(1, 2, 3)),
            bold: true,
            align: Align::Center,
            ..Style::default()
        };
        let child = style_for_element("span", &parent);
        assert_eq!(child.color, Some(Color::rgb(1, 2, 3)));
        assert!(child.bold);
        assert_eq!(child.align, Align::Center);
        assert_eq!(child.display, Display::Inline);

        let heading = style_for_element("h2", &parent);
        assert_eq!(heading.font_scale, 2);
        assert_eq!(heading.display, Display::Block);
    }

    #[test]
    fn parses_px_values() {
        assert_eq!(parse_px("12px"), Some(12));
        assert_eq!(parse_px(" 7 "), Some(7));
        assert_eq!(parse_px("12.9px"), Some(12));
        assert_eq!(parse_px(""), None);
        assert_eq!(parse_px("abc"), None);
        assert_eq!(parse_px("99999px"), Some(512));
    }
}
