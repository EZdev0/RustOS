use crate::desktop::app::{App, Event};
use crate::desktop::compositor::GraphicalCompositor;
use alloc::string::String;

pub struct NotepadApp {
    text: String,
    cursor_visible: bool,
    ticks: usize,
    scroll_y: isize,
}

impl Default for NotepadApp {
    fn default() -> Self {
        Self::new()
    }
}

impl NotepadApp {
    pub fn new() -> Self {
        Self {
            text: String::from("Welcome to RustOS Notepad!\nType something...\n\n"),
            cursor_visible: true,
            ticks: 0,
            scroll_y: -1,
        }
    }
}

impl App for NotepadApp {
    fn title(&self) -> &str {
        "Notepad"
    }

    fn update(&mut self) {
        self.ticks += 1;
        if self.ticks.is_multiple_of(20) {
            self.cursor_visible = !self.cursor_visible;
        }
    }

    fn draw(&mut self, compositor: &mut GraphicalCompositor, x: usize, y: usize, width: usize, height: usize) {
        let is_dark = crate::desktop::THEME.load(core::sync::atomic::Ordering::Relaxed) == 1;
        let (bg_r, bg_g, bg_b) = if is_dark { (30, 30, 35) } else { (255, 255, 255) };
        let (fg_r, fg_g, fg_b) = if is_dark { (220, 220, 220) } else { (0, 0, 0) };

        compositor.draw_rect(x, y, width, height, bg_r, bg_g, bg_b);

        let padding = 10;
        let max_lines = if height > 2 * padding { (height - 2 * padding) / 12 } else { 0 };
        let content_width = width.saturating_sub(10);
        let chars_per_line = if content_width > 2 * padding { (content_width - 2 * padding) / 8 } else { 1 };
        
        let mut visual_lines = 1;
        let mut current_line_len = 0;
        for c in self.text.chars() {
            if c == '\n' {
                visual_lines += 1;
                current_line_len = 0;
            } else {
                current_line_len += 1;
                if current_line_len > chars_per_line {
                    visual_lines += 1;
                    current_line_len = 1;
                }
            }
        }

        let max_scroll = if visual_lines > max_lines && max_lines > 0 {
            visual_lines - max_lines
        } else {
            0
        };

        if self.scroll_y == -1 {
            self.scroll_y = max_scroll as isize;
        } else {
            self.scroll_y = self.scroll_y.clamp(0, max_scroll as isize);
        }

        let start_line = self.scroll_y as usize;

        let mut cur_x = x + padding;
        let mut current_visual_line = 0;

        for c in self.text.chars() {
            let is_newline = c == '\n';
            let wrap = !is_newline && (cur_x + 8 > x + content_width - padding);
            
            if wrap {
                current_visual_line += 1;
                cur_x = x + padding;
            }

            if current_visual_line >= start_line {
                let cur_y = y + padding + (current_visual_line - start_line) * 12;
                if cur_y + 12 <= y + height - padding
                    && !is_newline {
                        compositor.draw_char(cur_x, cur_y, c, fg_r, fg_g, fg_b);
                        cur_x += 8;
                    }
            } else if !is_newline {
                cur_x += 8;
            }

            if is_newline {
                current_visual_line += 1;
                cur_x = x + padding;
            }
        }

        // Draw cursor
        if self.cursor_visible
            && current_visual_line >= start_line {
                let cur_y = y + padding + (current_visual_line - start_line) * 12;
                if cur_y + 12 <= y + height - padding {
                    compositor.draw_rect(cur_x, cur_y, 8, 10, fg_r, fg_g, fg_b);
                }
            }
        
        compositor.draw_scrollbar(x + width - 10, y, height, self.scroll_y as usize, max_scroll);
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::KeyPress(c) => {
                match c {
                    '\x08' => { let _ = self.text.pop(); }
                    '\n' | '\r' => self.text.push('\n'),
                    _ => self.text.push(c),
                }
                self.scroll_y = -1;
            },
            Event::MouseScroll { delta } => {
                
                self.scroll_y = self.scroll_y.saturating_add(delta as isize);
                if self.scroll_y < 0 { self.scroll_y = 0; }
            },
            _ => {}
        }
    }
}
