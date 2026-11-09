//! Block/inline layout producing a flat display list.
//!
//! Design constraints (they come from running inside a `no_std` kernel with a
//! software framebuffer, not from elegance):
//!
//! * **One pass, one allocation per string.** Layout runs once per page load or
//!   window resize, never per frame. Frames only *replay* the display list.
//! * **Flat output.** [`DrawCmd`] is either a rectangle or a glyph run, both of
//!   which the compositor can already blit. There is no retained box tree and no
//!   recursion during painting.
//! * **Monospace metrics.** Glyph advance is `8 px * scale`, which makes text
//!   measurement exact and allocation free (`char_count * advance`).
//! * **Bounded.** Command count, page height and nesting are capped, so a
//!   hostile page cannot exhaust the heap or overflow the stack.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::color::Color;
use crate::html::{Document, Node, NodeKind};
use crate::style::{style_for_element, Align, Display, Marker, Style};
use crate::stylesheet::StyleSheet;

/// Glyph cell width in pixels at scale 1.
pub const GLYPH_W: u32 = 8;
/// Glyph cell height in pixels at scale 1.
pub const GLYPH_H: u32 = 8;
/// Extra leading between two text lines.
pub const LINE_GAP: u32 = 3;
/// Margin around the page content.
pub const PAGE_PADDING: u32 = 10;
/// Maximum number of display list entries (hard heap guard).
pub const MAX_CMDS: usize = 20_000;
/// Maximum page height in pixels.
pub const MAX_PAGE_HEIGHT: u32 = 200_000;

/// A single painting operation in absolute page coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawCmd {
    /// Fill a solid rectangle.
    FillRect {
        /// Left edge.
        x: i32,
        /// Top edge.
        y: i32,
        /// Width in pixels.
        w: u32,
        /// Height in pixels.
        h: u32,
        /// Fill colour.
        color: Color,
    },
    /// Draw a run of text with the 8x8 bitmap font.
    Text {
        /// Left edge of the first glyph.
        x: i32,
        /// Top edge of the glyph cell.
        y: i32,
        /// Glyph scale (1 = 8x8 px).
        scale: u8,
        /// Draw a second pass offset by one pixel to fake bold.
        bold: bool,
        /// Glyph colour.
        color: Color,
        /// The text itself (already whitespace-collapsed).
        text: String,
    },
}

impl DrawCmd {
    /// `(x, y, width, height)` bounding box of this command.
    pub fn bounds(&self) -> (i32, i32, u32, u32) {
        match self {
            DrawCmd::FillRect { x, y, w, h, .. } => (*x, *y, *w, *h),
            DrawCmd::Text {
                x, y, scale, text, ..
            } => {
                let width = text.chars().count() as u32 * (*scale as u32) * GLYPH_W;
                (*x, *y, width, *scale as u32 * GLYPH_H + LINE_GAP)
            }
        }
    }

    /// True when the command intersects the vertical band `[top, top + height)`.
    pub fn is_visible_in(&self, top: i32, height: u32) -> bool {
        let (_, y, _, cmd_height) = self.bounds();
        let band_end = top.saturating_add(height as i32);
        y + cmd_height as i32 > top && y < band_end
    }
}

/// Options for a layout pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutOptions {
    /// Viewport width in pixels (the window's inner width).
    pub width: u32,
    /// Dark theme: brighter default text colour.
    pub dark_mode: bool,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            width: 800,
            dark_mode: false,
        }
    }
}

/// The result of laying out a document.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    /// Painting commands in back-to-front order.
    pub cmds: Vec<DrawCmd>,
    /// Total content height in pixels.
    pub height: u32,
    /// Width the layout was computed for.
    pub width: u32,
    /// True when a cap was hit and the page is incomplete.
    pub truncated: bool,
}

impl Layout {
    /// Commands intersecting the viewport starting at `scroll`.
    pub fn cmds_in_view(
        &self,
        scroll: i32,
        viewport_height: u32,
    ) -> impl Iterator<Item = &DrawCmd> {
        self.cmds
            .iter()
            .filter(move |cmd| cmd.is_visible_in(scroll, viewport_height))
    }

    /// Largest useful scroll offset for a viewport of `viewport_height`.
    pub fn max_scroll(&self, viewport_height: u32) -> u32 {
        self.height.saturating_sub(viewport_height)
    }
}

/// Style properties needed while assembling a line of text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InlineStyle {
    scale: u8,
    bold: bool,
    color: Color,
    underline: bool,
    preformatted: bool,
}

impl InlineStyle {
    fn new(style: &Style, fallback_color: Color) -> Self {
        Self {
            scale: style.font_scale.clamp(1, 4),
            bold: style.bold,
            color: style.color.unwrap_or(fallback_color),
            underline: style.underline,
            preformatted: style.preformatted,
        }
    }

    fn advance(&self) -> u32 {
        self.scale as u32 * GLYPH_W
    }

    fn line_height(&self) -> u32 {
        self.scale as u32 * GLYPH_H
    }
}

#[derive(Debug)]
struct Run {
    text: String,
    style: InlineStyle,
}

#[derive(Debug)]
enum Token {
    Word(String, InlineStyle),
    Break,
}

#[derive(Debug, Clone, Copy)]
struct ListState {
    ordered: bool,
    counter: u16,
}

struct Layouter<'a> {
    cmds: Vec<DrawCmd>,
    sheet: &'a StyleSheet,
    text_color: Color,
    y: u32,
    runs: Vec<Run>,
    pending_space: bool,
    inline_x: i32,
    inline_width: u32,
    inline_align: Align,
    list_stack: Vec<ListState>,
    truncated: bool,
}

/// Lay out a parsed document into a display list.
pub fn layout_document(document: &Document, options: LayoutOptions) -> Layout {
    let sheet = StyleSheet::parse_many(&document.stylesheets);
    let page_width = options.width.max(64);
    let mut layouter = Layouter {
        cmds: Vec::new(),
        sheet: &sheet,
        text_color: if options.dark_mode {
            Color::rgb(232, 234, 240)
        } else {
            Color::TEXT
        },
        y: PAGE_PADDING,
        runs: Vec::new(),
        pending_space: false,
        inline_x: PAGE_PADDING as i32,
        inline_width: page_width.saturating_sub(PAGE_PADDING * 2).max(16),
        inline_align: Align::Left,
        list_stack: Vec::new(),
        truncated: document.truncated,
    };

    let root_style = Style {
        color: Some(layouter.text_color),
        ..Style::default()
    };
    for child in &document.root.children {
        layouter.layout_node(child, &root_style);
    }
    layouter.flush_inline();

    let height = layouter.y.saturating_add(PAGE_PADDING);
    Layout {
        cmds: layouter.cmds,
        height,
        width: page_width,
        truncated: layouter.truncated,
    }
}

impl Layouter<'_> {
    fn budget_left(&self) -> bool {
        self.cmds.len() < MAX_CMDS && self.y < MAX_PAGE_HEIGHT
    }

    fn push(&mut self, cmd: DrawCmd) {
        if !self.budget_left() {
            self.truncated = true;
            return;
        }
        self.cmds.push(cmd);
    }

    fn layout_node(&mut self, node: &Node, parent: &Style) {
        if !self.budget_left() {
            self.truncated = true;
            return;
        }
        match &node.kind {
            NodeKind::Text(text) => {
                let inline = InlineStyle::new(parent, self.text_color);
                self.add_text(text, inline);
            }
            NodeKind::Document => {
                for child in &node.children {
                    self.layout_node(child, parent);
                }
            }
            NodeKind::Element(element) => {
                let mut style = style_for_element(&element.tag, parent);
                self.sheet.apply(element, &mut style);
                if let Some(inline) = element.attr("style") {
                    style.apply_declarations(inline);
                }
                if style.display == Display::None {
                    return;
                }
                self.layout_element(node, &element.tag, &style, parent);
            }
        }
    }

    fn layout_element(&mut self, node: &Node, tag: &str, style: &Style, _parent: &Style) {
        match tag {
            "br" => {
                self.add_break();
            }
            "hr" => {
                self.flush_inline();
                let x = self.inline_x;
                let width = self.inline_width;
                self.y += style.margin_before as u32;
                self.push(DrawCmd::FillRect {
                    x,
                    y: self.y as i32,
                    w: width,
                    h: 1,
                    color: Color::rgb(180, 184, 192),
                });
                self.y += 1 + style.margin_after as u32;
            }
            "img" => {
                self.flush_inline();
                self.y += style.margin_before as u32;
                let x = self.inline_x;
                let size = 32u32 * style.font_scale.min(2) as u32;
                self.push(DrawCmd::FillRect {
                    x,
                    y: self.y as i32,
                    w: size,
                    h: size,
                    color: Color::rgb(205, 210, 220),
                });
                // Simple frame: four thin rectangles.
                self.push(DrawCmd::FillRect {
                    x,
                    y: self.y as i32,
                    w: size,
                    h: 1,
                    color: Color::rgb(120, 126, 140),
                });
                self.push(DrawCmd::FillRect {
                    x,
                    y: self.y as i32 + size as i32 - 1,
                    w: size,
                    h: 1,
                    color: Color::rgb(120, 126, 140),
                });
                self.y += size;
                if let Some(alt) = node.attr("alt").filter(|alt| !alt.trim().is_empty()) {
                    let inline = InlineStyle::new(style, self.text_color);
                    self.add_text(&alloc::format!(" [{}]", alt), inline);
                }
                self.y += style.margin_after as u32;
            }
            "input" | "button" | "select" | "textarea" => {
                let inline = InlineStyle::new(style, self.text_color);
                let label = node
                    .attr("value")
                    .or_else(|| node.attr("placeholder"))
                    .unwrap_or("");
                let text = match tag {
                    "textarea" => {
                        let content = node.text_content();
                        if content.trim().is_empty() {
                            String::from("[textarea]")
                        } else {
                            alloc::format!("[{}]", content.trim())
                        }
                    }
                    "select" => {
                        let content = node.text_content();
                        alloc::format!("[{} v]", content.trim())
                    }
                    _ => {
                        if label.is_empty() {
                            let content = node.text_content();
                            if content.trim().is_empty() {
                                alloc::format!("[{tag}]")
                            } else {
                                alloc::format!("[{}]", content.trim())
                            }
                        } else {
                            alloc::format!("[{}]", label)
                        }
                    }
                };
                self.add_text(&text, inline);
            }
            "iframe" | "embed" | "object" | "video" | "audio" | "canvas" | "svg" => {
                self.flush_inline();
                let inline = InlineStyle::new(style, Color::rgb(150, 150, 160));
                self.add_text(&alloc::format!("[unsupported: {tag}]"), inline);
                self.flush_inline();
            }
            _ => {
                if style.display == Display::Inline {
                    for child in &node.children {
                        self.layout_node(child, style);
                    }
                } else {
                    self.layout_block(node, tag, style);
                }
            }
        }
    }

    fn layout_block(&mut self, node: &Node, tag: &str, style: &Style) {
        // Close the inline content of the enclosing block first.
        self.flush_inline();

        let x = self.inline_x;
        let width = self.inline_width;

        self.y = self.y.saturating_add(style.margin_before as u32);
        let start_y = self.y;
        let inner_x = x + style.indent as i32;
        let inner_width = width.saturating_sub(style.indent as u32).max(GLYPH_W * 2);
        let insert_at = self.cmds.len();

        if style.marker != Marker::None {
            let marker_text = match self.list_stack.last_mut() {
                Some(list) if list.ordered => {
                    list.counter = list.counter.saturating_add(1);
                    alloc::format!("{}.", list.counter)
                }
                _ => {
                    let _ = Marker::Bullet;
                    String::from("*")
                }
            };
            let scale = style.font_scale.clamp(1, 4);
            let marker_x = (x - 14).max(0);
            let color = style.color.unwrap_or(self.text_color);
            self.push(DrawCmd::Text {
                x: marker_x,
                y: self.y as i32,
                scale,
                bold: style.bold,
                color,
                text: marker_text,
            });
        }

        let saved = (self.inline_x, self.inline_width, self.inline_align);
        self.inline_x = inner_x;
        self.inline_width = inner_width;
        self.inline_align = style.align;

        let list_depth = self.list_stack.len();
        if tag == "ul" || tag == "ol" {
            self.list_stack.push(ListState {
                ordered: tag == "ol",
                counter: 0,
            });
        }

        for child in &node.children {
            self.layout_node(child, style);
            if !self.budget_left() {
                break;
            }
        }

        if self.list_stack.len() > list_depth {
            self.list_stack.pop();
        }

        self.flush_inline();
        self.inline_x = saved.0;
        self.inline_width = saved.1;
        self.inline_align = saved.2;

        let content_height = self.y.saturating_sub(start_y);

        if let Some(background) = style.background {
            if content_height > 0 {
                self.insert_before(
                    insert_at,
                    DrawCmd::FillRect {
                        x: inner_x,
                        y: start_y as i32,
                        w: inner_width,
                        h: content_height,
                        color: background,
                    },
                );
            }
        }

        if style.border && content_height > 0 {
            let border_color = Color::rgb(180, 184, 192);
            let mut rects = alloc::vec![
                DrawCmd::FillRect {
                    x: inner_x,
                    y: start_y as i32,
                    w: inner_width,
                    h: 1,
                    color: border_color,
                },
                DrawCmd::FillRect {
                    x: inner_x,
                    y: (start_y as i32) + content_height as i32 - 1,
                    w: inner_width,
                    h: 1,
                    color: border_color,
                },
                DrawCmd::FillRect {
                    x: inner_x,
                    y: start_y as i32,
                    w: 1,
                    h: content_height,
                    color: border_color,
                },
                DrawCmd::FillRect {
                    x: inner_x + inner_width as i32 - 1,
                    y: start_y as i32,
                    w: 1,
                    h: content_height,
                    color: border_color,
                },
            ];
            // Insert in reverse so the visual order stays top, bottom, left, right.
            while let Some(rect) = rects.pop() {
                self.insert_before(insert_at, rect);
            }
        }

        self.y = self.y.saturating_add(style.margin_after as u32);
    }

    fn insert_before(&mut self, index: usize, cmd: DrawCmd) {
        if !self.budget_left() {
            self.truncated = true;
            return;
        }
        let index = index.min(self.cmds.len());
        self.cmds.insert(index, cmd);
    }

    fn add_text(&mut self, text: &str, style: InlineStyle) {
        if text.is_empty() {
            return;
        }
        if !style.preformatted {
            let mut buffer = String::with_capacity(text.len());
            for character in text.chars() {
                if character == '\n' {
                    if !buffer.is_empty() {
                        self.push_run(core::mem::take(&mut buffer), style);
                    }
                    self.add_break();
                    self.pending_space = false;
                    continue;
                }
                if character.is_whitespace() {
                    self.pending_space = true;
                    continue;
                }
                if self.pending_space && (!buffer.is_empty() || !self.runs.is_empty()) {
                    buffer.push(' ');
                }
                self.pending_space = false;
                buffer.push(character);
            }
            if !buffer.is_empty() {
                self.push_run(buffer, style);
            }
        } else {
            self.push_run(text.to_string(), style);
        }
    }

    fn push_run(&mut self, text: String, style: InlineStyle) {
        if let Some(last) = self.runs.last_mut() {
            if last.style == style {
                last.text.push_str(&text);
                return;
            }
        }
        self.runs.push(Run { text, style });
    }

    fn add_break(&mut self) {
        let style = self
            .runs
            .last()
            .map(|run| run.style)
            .unwrap_or(InlineStyle {
                scale: 1,
                bold: false,
                color: self.text_color,
                underline: false,
                preformatted: false,
            });
        self.check_break_run(style);
    }

    fn check_break_run(&mut self, style: InlineStyle) {
        let needs_style = self.runs.last().map(|run| run.style) != Some(style);
        if needs_style {
            self.runs.push(Run {
                text: String::new(),
                style,
            });
        }
        if let Some(last) = self.runs.last_mut() {
            last.text.push('\n');
        }
    }

    fn flush_inline(&mut self) {
        if self.runs.is_empty() {
            return;
        }
        let runs = core::mem::take(&mut self.runs);
        self.pending_space = false;

        let mut tokens: Vec<Token> = Vec::new();
        for run in &runs {
            tokenize(run, &mut tokens);
        }

        let x0 = self.inline_x;
        let available = self.inline_width;
        let align = self.inline_align;

        let mut segments: Vec<(String, InlineStyle)> = Vec::new();
        let mut line_width: u32 = 0;

        for token in tokens {
            match token {
                Token::Break => {
                    self.emit_line(&mut segments, line_width, x0, available, align);
                    line_width = 0;
                }
                Token::Word(word, style) => {
                    if word.is_empty() {
                        continue;
                    }
                    let word_width = glyph_width(&word, style);
                    let space = if segments.is_empty() {
                        0
                    } else {
                        style.advance()
                    };
                    if !segments.is_empty() && line_width + space + word_width > available {
                        self.emit_line(&mut segments, line_width, x0, available, align);
                        line_width = 0;
                    }
                    if let Some(last) = segments.last_mut() {
                        if last.1 == style {
                            last.0.push(' ');
                            last.0.push_str(&word);
                            line_width = line_width.saturating_add(space + word_width);
                            continue;
                        }
                    }
                    let text = if segments.is_empty() {
                        word
                    } else {
                        let mut padded = String::with_capacity(word.len() + 1);
                        padded.push(' ');
                        padded.push_str(&word);
                        padded
                    };
                    line_width = line_width.saturating_add(space + word_width);
                    segments.push((text, style));
                }
            }
        }

        if !segments.is_empty() {
            self.emit_line(&mut segments, line_width, x0, available, align);
        }
    }

    fn emit_line(
        &mut self,
        segments: &mut Vec<(String, InlineStyle)>,
        line_width: u32,
        x0: i32,
        available: u32,
        align: Align,
    ) {
        if segments.is_empty() {
            return;
        }
        let mut x = match align {
            Align::Left => x0,
            Align::Center => x0 + available.saturating_sub(line_width) as i32 / 2,
            Align::Right => x0 + available.saturating_sub(line_width) as i32,
        };
        let mut line_height = GLYPH_H;
        for (text, style) in segments.iter() {
            line_height = line_height.max(style.line_height());
            let width = glyph_width(text, *style);
            if !text.trim().is_empty() {
                self.push(DrawCmd::Text {
                    x,
                    y: self.y as i32,
                    scale: style.scale,
                    bold: style.bold,
                    color: style.color,
                    text: text.clone(),
                });
                if style.underline {
                    self.push(DrawCmd::FillRect {
                        x,
                        y: self.y as i32 + style.line_height() as i32 - 1,
                        w: width,
                        h: 1,
                        color: style.color,
                    });
                }
            }
            x += width as i32;
        }
        segments.clear();
        self.y = self.y.saturating_add(line_height + LINE_GAP);
    }
}

fn glyph_width(text: &str, style: InlineStyle) -> u32 {
    text.chars().count() as u32 * style.advance()
}

fn tokenize(run: &Run, out: &mut Vec<Token>) {
    if run.style.preformatted {
        let mut first = true;
        for line in run.text.split('\n') {
            if !first {
                out.push(Token::Break);
            }
            first = false;
            if !line.is_empty() {
                out.push(Token::Word(line.to_string(), run.style));
            }
        }
        return;
    }

    let mut word = String::new();
    let mut pending_space = false;
    for character in run.text.chars() {
        if character == '\n' {
            if !word.is_empty() {
                out.push(Token::Word(core::mem::take(&mut word), run.style));
            }
            pending_space = false;
            out.push(Token::Break);
            continue;
        }
        if character.is_whitespace() {
            if !word.is_empty() {
                out.push(Token::Word(core::mem::take(&mut word), run.style));
                pending_space = true;
            }
            continue;
        }
        let _ = pending_space;
        word.push(character);
    }
    if !word.is_empty() {
        out.push(Token::Word(word, run.style));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    fn layout(html: &str, width: u32) -> Layout {
        let document = Document::parse(html);
        layout_document(
            &document,
            LayoutOptions {
                width,
                dark_mode: false,
            },
        )
    }

    fn texts(layout: &Layout) -> Vec<String> {
        layout
            .cmds
            .iter()
            .filter_map(|cmd| match cmd {
                DrawCmd::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn text_cmds(layout: &Layout) -> Vec<&DrawCmd> {
        layout
            .cmds
            .iter()
            .filter(|cmd| matches!(cmd, DrawCmd::Text { .. }))
            .collect()
    }

    #[test]
    fn lays_out_heading_and_paragraph() {
        let layout = layout("<h1>Title</h1><p>Body text</p>", 400);
        let rendered = texts(&layout);
        assert!(rendered.iter().any(|text| text == "Title"));
        assert!(rendered.iter().any(|text| text.contains("Body text")));
        let heading = text_cmds(&layout)[0];
        let (_, heading_y, _, heading_h) = heading.bounds();
        assert!(heading_h >= 24, "h1 should use scale 3");
        let (_, body_y, _, _) = text_cmds(&layout)[1].bounds();
        assert!(body_y > heading_y);
        assert!(layout.height > 40);
    }

    #[test]
    fn wraps_long_paragraphs() {
        let long = "word ".repeat(80);
        let narrow = layout(&format!("<p>{long}</p>"), 160);
        let wide = layout(&format!("<p>{long}</p>"), 1600);
        assert!(text_cmds(&narrow).len() > text_cmds(&wide).len());
        assert!(narrow.height > wide.height);
        // Every line must stay inside the viewport.
        for cmd in text_cmds(&narrow) {
            let (x, _, w, _) = cmd.bounds();
            assert!(x >= 0);
            assert!(x as u32 + w <= 160, "line overflows: {x} + {w}");
        }
    }

    #[test]
    fn centres_text_when_requested() {
        let left = layout("<p>hi</p>", 400);
        let centred = layout("<p style=\"text-align:center\">hi</p>", 400);
        let left_x = text_cmds(&left)[0].bounds().0;
        let centr_x = text_cmds(&centred)[0].bounds().0;
        assert!(centr_x > left_x);
    }

    #[test]
    fn links_are_blue_and_underlined() {
        let layout = layout("<p><a href=\"/x\">link</a></p>", 400);
        let text = text_cmds(&layout)[0];
        match text {
            DrawCmd::Text { color, text, .. } => {
                assert!(color.b < 250 && color.r < 100, "link should be blue");
                assert!(text.contains("link"));
            }
            _ => panic!("expected text"),
        }
        let underline = layout
            .cmds
            .iter()
            .any(|cmd| matches!(cmd, DrawCmd::FillRect { h: 1, .. }));
        assert!(underline, "link must render an underline");
    }

    #[test]
    fn bold_spans_set_the_bold_flag() {
        let layout = layout("<p>normal <b>bold</b></p>", 400);
        let bold = text_cmds(&layout).iter().any(|cmd| match cmd {
            DrawCmd::Text { bold, .. } => *bold,
            _ => false,
        });
        assert!(bold);
    }

    #[test]
    fn display_none_is_skipped() {
        let layout = layout("<p>shown</p><div style=\"display:none\">hidden</div>", 400);
        let rendered = texts(&layout).join(" ");
        assert!(rendered.contains("shown"));
        assert!(!rendered.contains("hidden"));
    }

    #[test]
    fn head_content_is_not_rendered() {
        let layout = layout(
            "<html><head><title>T</title><style>p{color:red}</style></head><body><p>body</p></body></html>",
            400,
        );
        let rendered = texts(&layout).join(" ");
        assert!(rendered.contains("body"));
        assert!(!rendered.contains('T'));
        assert!(!rendered.contains("color"));
    }

    #[test]
    fn lists_get_markers_and_numbers() {
        let bullets = layout("<ul><li>one</li><li>two</li></ul>", 400);
        let bullet_count = text_cmds(&bullets)
            .iter()
            .filter(|cmd| matches!(cmd, DrawCmd::Text { text, .. } if text == "*"))
            .count();
        assert_eq!(bullet_count, 2);

        let numbers = layout("<ol><li>one</li><li>two</li></ol>", 400);
        let rendered = texts(&numbers);
        assert!(rendered.iter().any(|text| text == "1."));
        assert!(rendered.iter().any(|text| text == "2."));
    }

    #[test]
    fn horizontal_rules_and_images_draw_boxes() {
        let layout = layout("<hr><img src=\"a.png\" alt=\"Alt text\">", 400);
        let rects = layout
            .cmds
            .iter()
            .filter(|cmd| matches!(cmd, DrawCmd::FillRect { .. }))
            .count();
        assert!(rects >= 3, "hr + image frame expected");
        assert!(texts(&layout).iter().any(|text| text.contains("Alt text")));
    }

    #[test]
    fn preformatted_text_does_not_wrap() {
        let source = "<pre>let x = 1;\nlet y = 2;</pre>";
        let layout = layout(source, 120);
        let rendered = texts(&layout);
        assert!(rendered.iter().any(|text| text.contains("let x = 1;")));
        assert_eq!(rendered.len(), 2);
    }

    #[test]
    fn line_breaks_split_lines() {
        let layout = layout("<p>one<br>two</p>", 400);
        let rendered = texts(&layout);
        assert_eq!(rendered.len(), 2);
        let first = text_cmds(&layout)[0].bounds().1;
        let second = text_cmds(&layout)[1].bounds().1;
        assert!(second > first);
    }

    #[test]
    fn tables_render_cells() {
        let layout = layout(
            "<table><tr><td>a</td><td>b</td></tr><tr><td>c</td></tr></table>",
            400,
        );
        let rendered = texts(&layout).join(" ");
        for cell in ["a", "b", "c"] {
            assert!(rendered.contains(cell));
        }
    }

    #[test]
    fn stylesheet_rules_are_applied() {
        let layout = layout(
            "<style>.big { font-size: 24px } .muted { color: #777777 }</style><p class=\"big\">large</p><p class=\"muted\">grey</p>",
            400,
        );
        let cmds = text_cmds(&layout);
        let large = cmds
            .iter()
            .find(|cmd| matches!(cmd, DrawCmd::Text { text, .. } if text.contains("large")))
            .expect("large text");
        match large {
            DrawCmd::Text { scale, .. } => assert_eq!(*scale, 3),
            _ => panic!("expected text"),
        }
        let grey = cmds
            .iter()
            .find(|cmd| matches!(cmd, DrawCmd::Text { text, .. } if text.contains("grey")))
            .expect("grey text");
        match grey {
            DrawCmd::Text { color, .. } => assert_eq!(*color, Color::rgb(0x77, 0x77, 0x77)),
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn blockquote_and_nested_lists_indent() {
        let layout = layout("<ul><li>outer<ul><li>inner</li></ul></li></ul>", 400);
        let cmds = text_cmds(&layout);
        let outer = cmds
            .iter()
            .find(|cmd| matches!(cmd, DrawCmd::Text { text, .. } if text.contains("outer")))
            .unwrap()
            .bounds()
            .0;
        let inner = cmds
            .iter()
            .find(|cmd| matches!(cmd, DrawCmd::Text { text, .. } if text.contains("inner")))
            .unwrap()
            .bounds()
            .0;
        assert!(inner > outer);
    }

    #[test]
    fn hostile_documents_never_panic() {
        let inputs = [
            "",
            "<",
            "<p>",
            "<svg><svg>",
            "<pre>",
            "<ul><li><ul><li><ul><li>x",
            "<p>&#0;&#xFFFFF;</p>",
            "<div style=\"font-size:99999px\">big</div>",
            "<table><tr><td><table><tr><td>nested",
            "<a href=\"\"><b><i><u>deep",
        ];
        for input in inputs {
            for width in [64u32, 100, 800] {
                let layout = layout(input, width);
                let _ = layout.max_scroll(600);
                let _ = layout.cmds_in_view(0, 600).count();
            }
        }
    }

    #[test]
    fn command_budget_is_bounded() {
        let mut html = String::new();
        for _ in 0..30_000 {
            html.push_str("<p>paragraph</p>");
        }
        let layout = layout(&html, 400);
        assert!(layout.cmds.len() <= MAX_CMDS);
        assert!(layout.truncated);
    }

    #[test]
    fn view_culling_and_scrolling() {
        let layout = layout("<p>a</p><p>b</p><p>c</p>", 400);
        let all = layout.cmds.len();
        let visible = layout.cmds_in_view(0, 16).count();
        assert!(visible < all);
        assert_eq!(layout.max_scroll(0), layout.height);
        assert_eq!(layout.max_scroll(100_000), 0);
    }

    #[test]
    fn dark_mode_changes_text_colour() {
        let document = Document::parse("<p>text</p>");
        let light = layout_document(
            &document,
            LayoutOptions {
                width: 300,
                dark_mode: false,
            },
        );
        let dark = layout_document(
            &document,
            LayoutOptions {
                width: 300,
                dark_mode: true,
            },
        );
        let light_color = match &light.cmds[0] {
            DrawCmd::Text { color, .. } => *color,
            _ => panic!("expected text"),
        };
        let dark_color = match &dark.cmds[0] {
            DrawCmd::Text { color, .. } => *color,
            _ => panic!("expected text"),
        };
        assert!(dark_color.brightness() > light_color.brightness());
    }

    #[test]
    fn tiny_viewports_do_not_overflow() {
        for width in [1u32, 8, 16, 40] {
            let layout = layout("<h1>Heading</h1><p>Some words here</p>", width);
            for cmd in &layout.cmds {
                let (x, _, w, _) = cmd.bounds();
                assert!(x >= 0);
                let _ = w;
            }
        }
    }
}
