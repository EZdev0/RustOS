use crate::desktop::app::{App, Event};
use crate::desktop::compositor::GraphicalCompositor;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub struct TerminalApp {
    output: Vec<String>,
    input_buffer: String,
    cursor_visible: bool,
    ticks: usize,
}

impl TerminalApp {
    pub fn new() -> Self {
        let mut output = Vec::new();
        output.push(String::from("Welcome to RustOS Terminal!"));
        output.push(String::from("Type 'help' for commands."));
        Self {
            output,
            input_buffer: String::new(),
            cursor_visible: true,
            ticks: 0,
        }
    }

    fn execute_command(&mut self) {
        let cmd = self.input_buffer.trim().to_string();
        self.output.push(String::from("> ") + &cmd);
        
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            self.input_buffer.clear();
            return;
        }

        match parts[0] {
            "help" => {
                self.output.push(String::from("Available commands: help, clear, echo"));
            }
            "clear" => {
                self.output.clear();
            }
            "echo" => {
                if parts.len() > 1 {
                    self.output.push(parts[1..].join(" "));
                }
            }
            _ => {
                self.output.push(String::from("Unknown command: ") + parts[0]);
            }
        }
        self.input_buffer.clear();
    }
}

impl App for TerminalApp {
    fn title(&self) -> &str {
        "Terminal"
    }

    fn update(&mut self) {
        self.ticks += 1;
        if self.ticks % 20 == 0 {
            self.cursor_visible = !self.cursor_visible;
        }
    }

    fn draw(&mut self, compositor: &mut GraphicalCompositor, x: usize, y: usize, width: usize, height: usize) {
        // Schwarzer Hintergrund für Terminal
        compositor.draw_rect(x, y, width, height, 0, 0, 0);

        let padding = 10;
        let mut cur_y = y + padding;
        let max_lines = if height > 2 * padding { (height - 2 * padding) / 12 } else { 0 };

        let total_lines = self.output.len() + 1; // output lines + input line
        
        let start_line = if total_lines > max_lines && max_lines > 0 {
            total_lines - max_lines
        } else {
            0
        };

        // Output zeichnen (Grün)
        for (i, line) in self.output.iter().enumerate() {
            if i >= start_line {
                let mut cur_x = x + padding;
                for c in line.chars() {
                    compositor.draw_char(cur_x, cur_y, c, 0, 255, 0);
                    cur_x += 8;
                }
                cur_y += 12;
            }
        }

        // Input Buffer und Cursor zeichnen
        if total_lines >= start_line && start_line <= self.output.len() {
            let mut cur_x = x + padding;
            compositor.draw_char(cur_x, cur_y, '>', 0, 255, 0);
            cur_x += 16; // Abstand nach dem Prompt

            for c in self.input_buffer.chars() {
                compositor.draw_char(cur_x, cur_y, c, 0, 255, 0);
                cur_x += 8;
            }

            if self.cursor_visible {
                compositor.draw_rect(cur_x, cur_y, 8, 10, 0, 255, 0);
            }
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::KeyPress(c) => {
                if c == '\x08' {
                    let _ = self.input_buffer.pop();
                } else if c == '\n' || c == '\r' {
                    self.execute_command();
                } else {
                    self.input_buffer.push(c);
                }
            }
            _ => {}
        }
    }
}
