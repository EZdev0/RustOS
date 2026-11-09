//! The RustOS web browser front-end.
//!
//! All parsing, styling and layout live in the `rustos-web` crate so they can
//! be unit tested on the host. This file only does three things:
//!
//! 1. drive one `PageEngine` (request -> chunks -> document),
//! 2. keep the scroll offset sane,
//! 3. replay the cached display list into the compositor.
//!
//! Replaying is O(visible commands), never O(document): that is what keeps
//! scrolling smooth even on a pure software framebuffer.

use crate::desktop::app::{App, Event};
use crate::desktop::compositor::GraphicalCompositor;
use alloc::string::String;
use alloc::vec::Vec;
use rustos_web::layout::DrawCmd;
use rustos_web::{FetchState, LayoutOptions, PageEngine, Url};

/// Height of the URL/status chrome at the top of the window.
const TOOLBAR_HEIGHT: usize = 44;
/// Cap for a single page body (the kernel heap is 16 MiB in total).
const MAX_PAGE_BYTES: usize = 1_500_000;
/// Chunks consumed per `update()` call so the executor stays responsive.
const CHUNKS_PER_UPDATE: usize = 16;

/// Static host table used until a DNS client exists.
///
/// These entries can drift; the URL bar accepts a numeric IP at any time.
pub const HOSTS: &[(&str, [u8; 4])] = &[
    ("example.com", [93, 184, 216, 34]),
    ("www.example.com", [93, 184, 216, 34]),
    ("localhost", [10, 0, 2, 15]),
];

pub struct BrowserApp {
    url_bar: String,
    engine: Option<PageEngine>,
    pending_url: Option<Url>,
    /// URL whose request bytes were already handed to the network stack.
    sent_url: Option<Url>,
    scroll_y: i32,
    status: String,
    history: Vec<String>,
    show_start_page: bool,
}

impl Default for BrowserApp {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserApp {
    pub fn new() -> Self {
        Self {
            url_bar: String::from("http://example.com/"),
            engine: None,
            pending_url: None,
            sent_url: None,
            scroll_y: 0,
            status: String::from("Ready - type an address and press Enter"),
            history: Vec::new(),
            show_start_page: true,
        }
    }

    /// Start loading the address currently in the URL bar.
    fn navigate(&mut self) {
        match Url::parse(&self.url_bar) {
            Ok(url) => {
                if self.history.len() < 32 {
                    self.history.push(url.display());
                }
                self.url_bar = url.display();
                self.pending_url = Some(url);
            }
            Err(error) => {
                self.status = String::from(error.message());
            }
        }
    }

    /// Follow a link (used by the click handler and the terminal).
    pub fn open(&mut self, address: &str) {
        self.url_bar = String::from(address);
        self.navigate();
    }

    /// Scroll by `delta` lines, clamped to the document.
    fn scroll_by(&mut self, delta: i32, viewport: usize) {
        self.scroll_y = (self.scroll_y + delta).max(0);
        let max = self
            .engine
            .as_ref()
            .and_then(|engine| engine.cached_layout())
            .map(|layout| layout.max_scroll(viewport as u32))
            .unwrap_or(0);
        if self.scroll_y > max as i32 {
            self.scroll_y = max as i32;
        }
    }

    /// Resolve a host name through the static table (no DNS yet).
    fn resolve_host(url: &Url) -> Option<[u8; 4]> {
        if let Some(ip) = url.ipv4() {
            return Some(ip);
        }
        let host = url.host();
        HOSTS
            .iter()
            .find(|(name, _)| *name == host)
            .map(|(_, ip)| *ip)
    }

    fn status_line(&self) -> String {
        match &self.engine {
            None => self.status.clone(),
            Some(engine) => engine.status_text(),
        }
    }

    fn paint_command(
        compositor: &mut GraphicalCompositor,
        x: usize,
        content_top: usize,
        content_bottom: usize,
        width: usize,
        scroll: i32,
        cmd: &DrawCmd,
    ) {
        match cmd {
            DrawCmd::FillRect {
                x: cx,
                y: cy,
                w,
                h,
                color,
            } => {
                let screen_y = content_top as i32 + (*cy - scroll);
                if screen_y >= content_bottom as i32 || screen_y + *h as i32 <= content_top as i32 {
                    return;
                }
                let draw_y = screen_y.max(content_top as i32) as usize;
                let mut draw_h = *h as usize;
                if screen_y < content_top as i32 {
                    let skipped = (content_top as i32 - screen_y) as usize;
                    draw_h = draw_h.saturating_sub(skipped);
                }
                let max_h = content_bottom.saturating_sub(draw_y);
                draw_h = draw_h.min(max_h);
                if draw_h == 0 {
                    return;
                }
                let draw_x = x.saturating_add(*cx as usize);
                let draw_w = (*w as usize).min(width.saturating_sub(draw_x.saturating_sub(x)));
                if draw_w == 0 {
                    return;
                }
                compositor.draw_rect(draw_x, draw_y, draw_w, draw_h, color.r, color.g, color.b);
            }
            DrawCmd::Text {
                x: cx,
                y: cy,
                scale,
                bold,
                color,
                text,
            } => {
                let screen_y = content_top as i32 + (*cy - scroll);
                if screen_y < content_top as i32 || screen_y >= content_bottom as i32 {
                    return;
                }
                let draw_x = x.saturating_add(*cx as usize);
                if draw_x >= x.saturating_add(width) {
                    return;
                }
                compositor.draw_text(
                    draw_x,
                    screen_y as usize,
                    text,
                    *scale as usize,
                    *bold,
                    color.r,
                    color.g,
                    color.b,
                );
            }
        }
    }

    fn draw_start_page(
        &self,
        compositor: &mut GraphicalCompositor,
        x: usize,
        y: usize,
        height: usize,
        dark: bool,
    ) {
        let (title, text) = if dark {
            ((235, 238, 245), (170, 176, 190))
        } else {
            ((24, 28, 40), (96, 102, 116))
        };
        let mut line = y + 20;
        compositor.draw_text(
            x + 20,
            line,
            "RustOS Browser",
            3,
            true,
            title.0,
            title.1,
            title.2,
        );
        line += 40;
        for hint in [
            "HTML + CSS rendering engine (rustos-web)",
            "",
            "Type an address and press Enter:",
            "  http://example.com/",
            "  http://93.184.216.34/",
            "",
            "Keys:  Enter = load   Backspace = edit",
            "Mouse: wheel / arrow keys = scroll",
            "",
            "No TLS yet: https:// pages cannot be fetched.",
            "No DNS yet: use a name from the host table or a raw IPv4 address.",
        ] {
            if line + 12 > y + height {
                break;
            }
            compositor.draw_text(x + 20, line, hint, 1, false, text.0, text.1, text.2);
            line += 14;
        }
    }
}

impl App for BrowserApp {
    fn title(&self) -> &str {
        "Web Browser"
    }

    fn update(&mut self) {
        if let Some(url) = self.pending_url.take() {
            crate::network::cancel_http_request();
            self.sent_url = None;
            self.scroll_y = 0;
            self.show_start_page = false;
            self.status = String::from("Connecting...");
            self.engine = Some(PageEngine::start(url));
        }

        let mut unknown_host: Option<Url> = None;

        if let Some(engine) = self.engine.as_mut() {
            // 1. Hand the request to the network stack exactly once per URL.
            if engine.state() == FetchState::Requesting
                && self.sent_url.as_ref() != Some(engine.url())
            {
                let url = engine.url().clone();
                if let Some(request) = engine.take_request() {
                    match Self::resolve_host(&url) {
                        Some(ip) => {
                            crate::network::start_http_request_raw(
                                ip,
                                request.into_bytes(),
                                MAX_PAGE_BYTES,
                            );
                            self.sent_url = Some(url);
                        }
                        None => unknown_host = Some(url),
                    }
                }
            }

            // 2. Drain decoded chunks into the response parser.
            let mut consumed = 0;
            while consumed < CHUNKS_PER_UPDATE {
                match crate::network::poll_http_chunk() {
                    Some(chunk) => {
                        engine.feed(&chunk);
                        consumed += 1;
                    }
                    None => break,
                }
            }

            // 3. Detect a finished/aborted transfer.
            if engine.state() == FetchState::Receiving
                && (crate::network::http_failed()
                    || crate::network::http_state() == Some(crate::network::HttpState::Done))
            {
                engine.finish_stream();
            }
        }

        if let Some(url) = unknown_host {
            let page = rustos_web::error_page(
                "Unknown host",
                "This build has no DNS resolver yet.\n\
                 Enter a raw IPv4 address such as http://93.184.216.34/ \
                 or use a host from the built-in table.",
            );
            self.status = String::from("No DNS: use an IP address");
            self.engine = Some(PageEngine::from_html(url, &page));
        }

        if let Some(engine) = self.engine.as_ref() {
            self.status = engine.status_text();
        }
    }

    fn draw(
        &mut self,
        compositor: &mut GraphicalCompositor,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    ) {
        let dark = crate::desktop::is_dark();

        // ── Chrome ──────────────────────────────────────────────────────────
        let toolbar = if dark { (42, 44, 52) } else { (228, 230, 236) };
        compositor.draw_rect(x, y, width, TOOLBAR_HEIGHT, toolbar.0, toolbar.1, toolbar.2);

        let bar_x = x + 8;
        let bar_w = width.saturating_sub(16);
        let (url_bg, url_fg) = if dark {
            ((28, 30, 36), (225, 228, 236))
        } else {
            ((255, 255, 255), (24, 26, 32))
        };
        compositor.draw_rect(bar_x, y + 6, bar_w, 22, url_bg.0, url_bg.1, url_bg.2);
        compositor.draw_rect(bar_x, y + 6, bar_w, 1, 160, 166, 178);
        compositor.draw_rect(bar_x, y + 27, bar_w, 1, 160, 166, 178);

        // Show the tail of a long URL so the caret stays visible.
        let max_chars = bar_w.saturating_sub(8) / 8;
        let shown = if self.url_bar.chars().count() > max_chars {
            let skip = self.url_bar.chars().count() - max_chars;
            self.url_bar.chars().skip(skip).collect::<String>()
        } else {
            self.url_bar.clone()
        };
        compositor.draw_text(
            bar_x + 4,
            y + 13,
            &shown,
            1,
            false,
            url_fg.0,
            url_fg.1,
            url_fg.2,
        );

        let status = self.status_line();
        let status_color = if dark {
            (150, 156, 170)
        } else {
            (96, 100, 112)
        };
        let status_max = width.saturating_sub(20) / 8;
        let status_shown: String = status.chars().take(status_max).collect();
        compositor.draw_text(
            x + 8,
            y + 34,
            &status_shown,
            1,
            false,
            status_color.0,
            status_color.1,
            status_color.2,
        );

        // ── Content ─────────────────────────────────────────────────────────
        let content_top = y + TOOLBAR_HEIGHT;
        let content_height = height.saturating_sub(TOOLBAR_HEIGHT);
        if content_height == 0 {
            return;
        }
        let content_bottom = content_top + content_height;
        let page_bg = if dark { (26, 28, 34) } else { (255, 255, 255) };
        compositor.draw_rect(
            x,
            content_top,
            width,
            content_height,
            page_bg.0,
            page_bg.1,
            page_bg.2,
        );

        if self.show_start_page {
            self.draw_start_page(compositor, x, content_top, content_height, dark);
            return;
        }

        // Keep the scroll offset inside the document.
        let scroll = self.scroll_y;
        let mut max_scroll: u32 = 0;
        if let Some(engine) = self.engine.as_mut() {
            let options = LayoutOptions {
                width: width.saturating_sub(4) as u32,
                dark_mode: dark,
            };
            if let Some(layout) = engine.layout(options) {
                max_scroll = layout.max_scroll(content_height as u32);
                for cmd in layout.cmds_in_view(scroll, content_height as u32) {
                    Self::paint_command(
                        compositor,
                        x,
                        content_top,
                        content_bottom,
                        width,
                        scroll,
                        cmd,
                    );
                }
            } else {
                let message = match engine.state() {
                    FetchState::Failed => String::from("Transfer failed"),
                    FetchState::Requesting | FetchState::Receiving => String::from("Loading..."),
                    _ => String::from("Nothing to display"),
                };
                compositor.draw_text(x + 20, content_top + 20, &message, 2, false, 110, 116, 130);
            }
        }

        if self.scroll_y as u32 > max_scroll {
            self.scroll_y = max_scroll as i32;
        }
        compositor.draw_scrollbar(
            x + width.saturating_sub(10),
            content_top,
            content_height,
            self.scroll_y.max(0) as usize,
            max_scroll as usize,
        );
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::MouseScroll { delta } => {
                self.scroll_by(delta * 24, 400);
            }
            Event::MouseClick { .. } | Event::MouseLongPress { .. } => {}
            Event::KeyCode(_) => {}
            Event::KeyPress(character) => match character {
                '\n' => self.navigate(),
                '\x08' => {
                    self.url_bar.pop();
                }
                other if !other.is_control() && self.url_bar.chars().count() < 180 => {
                    self.url_bar.push(other);
                }
                _ => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_literal_and_tabled_hosts() {
        let literal = Url::parse("http://93.184.216.34/").expect("url");
        assert_eq!(BrowserApp::resolve_host(&literal), Some([93, 184, 216, 34]));
        let tabled = Url::parse("http://example.com/").expect("url");
        assert_eq!(BrowserApp::resolve_host(&tabled), Some([93, 184, 216, 34]));
        let unknown = Url::parse("http://does-not-exist.invalid/").expect("url");
        assert_eq!(BrowserApp::resolve_host(&unknown), None);
    }
}
