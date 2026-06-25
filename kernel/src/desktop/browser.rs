use crate::desktop::app::{App, Event};
use crate::desktop::compositor::GraphicalCompositor;
use alloc::string::String;
use alloc::vec::Vec;

pub struct BrowserApp {
    url_bar: String,
    content: String,
    scroll_y: isize,
    html_elements: Vec<HtmlElement>,
}

enum HtmlElement {
    Text(String),
    Heading(String),
    Link(String), // Text
}

impl Default for BrowserApp {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserApp {
    pub fn new() -> Self {
        Self {
            url_bar: String::from("http://localhost"),
            content: String::from("<h1>Welcome to RustOS Browser</h1><p>Loading...</p>"),
            scroll_y: 0,
            html_elements: Vec::new(),
        }
    }

    fn parse_dummy_html(&mut self) {
        self.html_elements.clear();
        
        let mut in_tag = false;
        let mut current_text = String::new();
        let mut current_tag = String::new();
        let mut active_style = "text";
        
        for c in self.content.chars() {
            if c == '<' {
                if !current_text.trim().is_empty() {
                    match active_style {
                        "h1" => self.html_elements.push(HtmlElement::Heading(current_text.clone())),
                        "a" => self.html_elements.push(HtmlElement::Link(current_text.clone())),
                        _ => self.html_elements.push(HtmlElement::Text(current_text.clone())),
                    }
                }
                current_text.clear();
                in_tag = true;
            } else if c == '>' {
                in_tag = false;
                let tag_lower = current_tag.to_lowercase();
                if tag_lower == "h1" { active_style = "h1"; }
                else if tag_lower == "a" { active_style = "a"; }
                else if tag_lower.starts_with('/') { active_style = "text"; }
                current_tag.clear();
            } else {
                if in_tag {
                    current_tag.push(c);
                } else {
                    current_text.push(c);
                }
            }
        }
        
        if !current_text.trim().is_empty() {
            self.html_elements.push(HtmlElement::Text(current_text));
        }
    }
}

impl App for BrowserApp {
    fn title(&self) -> &str {
        "Web Browser"
    }

    fn update(&mut self) {
        if self.html_elements.is_empty() && self.content == "<h1>Welcome to RustOS Browser</h1><p>Loading...</p>" {
            // Trigger HTTP request to example.com (93.184.216.34)
            crate::network::start_http_request([93, 184, 216, 34]);
            self.content = String::from("<h1>Loading...</h1><p>Fetching example.com...</p>");
            self.parse_dummy_html();
        }

        if let Some(res) = crate::network::get_http_response() {
            // Parse HTTP body simply by stripping headers (double CRLF)
            if let Some(body_idx) = res.find("\r\n\r\n") {
                self.content = String::from(&res[body_idx + 4..]);
            } else {
                self.content = res;
            }
            self.parse_dummy_html();
        }
    }

    fn draw(&mut self, compositor: &mut GraphicalCompositor, x: usize, y: usize, width: usize, height: usize) {
        // Draw Toolbar
        compositor.draw_rect(x, y, width, 40, 200, 200, 200);
        
        // Draw URL bar
        compositor.draw_rect(x + 10, y + 10, width - 20, 20, 255, 255, 255);
        
        let mut cx = x + 15;
        for c in self.url_bar.chars() {
            compositor.draw_char(cx, y + 16, c, 0, 0, 0);
            cx += 8;
        }

        // Draw background
        let content_y = y + 40;
        let content_height = height.saturating_sub(40);
        compositor.draw_rect(x, content_y, width, content_height, 255, 255, 255);

        // Render HTML Elements (stub)
        let mut draw_y = content_y as isize + 10 - self.scroll_y;
        
        for el in &self.html_elements {
            if draw_y > content_y as isize + content_height as isize {
                break;
            }
            if draw_y < content_y as isize {
                // skip
            } else {
                match el {
                    HtmlElement::Heading(text) => {
                        let mut px = x + 20;
                        for c in text.chars() {
                            compositor.draw_char(px, draw_y as usize, c, 50, 50, 150);
                            px += 8;
                        }
                        draw_y += 30;
                    },
                    HtmlElement::Text(text) => {
                        let mut px = x + 20;
                        for c in text.chars() {
                            compositor.draw_char(px, draw_y as usize, c, 0, 0, 0);
                            px += 8;
                        }
                        draw_y += 20;
                    },
                    HtmlElement::Link(text) => {
                        let mut px = x + 20;
                        for c in text.chars() {
                            compositor.draw_char(px, draw_y as usize, c, 0, 0, 255);
                            px += 8;
                        }
                        draw_y += 20;
                    }
                }
            }
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::MouseScroll { delta } => {
                self.scroll_y += (delta * 20) as isize;
                if self.scroll_y < 0 {
                    self.scroll_y = 0;
                }
            }
            Event::KeyPress(c) => {
                if c == '\x08' {
                    self.url_bar.pop();
                } else if c != '\n' {
                    self.url_bar.push(c);
                } else {
                    // Enter pressed
                    self.content = String::from("<h1>Navigating...</h1><p>Wait</p>");
                    self.html_elements.clear();
                }
            }
            _ => {}
        }
    }
}
