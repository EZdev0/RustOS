use crate::desktop::app::{App, Event};
use crate::desktop::compositor::GraphicalCompositor;
use alloc::string::String;

pub struct NotepadApp {
    text: String,
    cursor_visible: bool,
    ticks: usize,
}

impl NotepadApp {
    pub fn new() -> Self {
        Self {
            text: String::from("Welcome to RustOS Notepad!\nType something...\n\n"),
            cursor_visible: true,
            ticks: 0,
        }
    }
}

impl App for NotepadApp {
    fn title(&self) -> &str {
        "Notepad"
    }

    fn update(&mut self) {
        self.ticks += 1;
        if self.ticks % 20 == 0 {
            self.cursor_visible = !self.cursor_visible;
        }
    }

    fn draw(&mut self, compositor: &mut GraphicalCompositor, x: usize, y: usize, width: usize, height: usize) {
        // Draw white background for notepad
        compositor.draw_rect(x, y, width, height, 255, 255, 255);

        let padding = 10;
        let mut cur_x = x + padding;
        let mut cur_y = y + padding;

        for c in self.text.chars() {
            if c == '\n' {
                cur_y += 12;
                cur_x = x + padding;
            } else {
                if cur_x + 8 > x + width - padding {
                    cur_y += 12;
                    cur_x = x + padding;
                }
                // Black text
                compositor.draw_char(cur_x, cur_y, c, 0, 0, 0);
                cur_x += 8;
            }
        }

        // Draw cursor
        if self.cursor_visible {
            compositor.draw_rect(cur_x, cur_y, 8, 10, 0, 0, 0);
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::KeyPress(c) => {
                if c == '\x08' {
                    let _ = self.text.pop();
                } else if c == '\n' || c == '\r' {
                    self.text.push('\n');
                } else {
                    self.text.push(c);
                }
            }
            _ => {}
        }
    }
}
