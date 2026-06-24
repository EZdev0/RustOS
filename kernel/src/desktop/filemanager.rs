use crate::desktop::app::{App, Event};
use crate::desktop::compositor::GraphicalCompositor;
use alloc::vec::Vec;
use alloc::string::String;

fn draw_string(compositor: &mut GraphicalCompositor, mut x: usize, y: usize, s: &str, r: u8, g: u8, b: u8) {
    for c in s.chars() {
        compositor.draw_char(x, y, c, r, g, b);
        x += 8;
    }
}

pub struct FileManagerApp {
    items: Vec<(String, bool)>,
    new_file_name: String,
    is_creating_file: bool,
    ticks: usize,
    scroll_y: isize,
}

impl Default for FileManagerApp {
    fn default() -> Self {
        Self::new()
    }
}

impl FileManagerApp {
    pub fn new() -> Self {
        FileManagerApp {
            items: crate::fs::RAM_FS.list_dir("").unwrap_or_default(),
            new_file_name: String::new(),
            is_creating_file: false,
            ticks: 0,
            scroll_y: 0,
        }
    }
}

impl App for FileManagerApp {
    fn title(&self) -> &str {
        "Finder"
    }

    fn update(&mut self) {
        self.ticks += 1;
        // Optimization: Only scan the filesystem once per second to prevent 60FPS I/O stuttering
        if self.ticks.is_multiple_of(60) {
            self.items = crate::fs::RAM_FS.list_dir("").unwrap_or_default();
        }
    }

    fn draw(&mut self, compositor: &mut GraphicalCompositor, x: usize, y: usize, width: usize, height: usize) {
        let is_dark = crate::desktop::THEME.load(core::sync::atomic::Ordering::Relaxed) == 1;
        let (bg_r, bg_g, bg_b) = if is_dark { (30, 30, 35) } else { (240, 240, 245) };
        let (sb_r, sb_g, sb_b) = if is_dark { (40, 40, 45) } else { (220, 220, 230) };
        let (tb_r, tb_g, tb_b) = if is_dark { (45, 45, 50) } else { (230, 230, 240) };
        let (text_r, text_g, text_b) = if is_dark { (220, 220, 220) } else { (50, 50, 60) };
        let (sub_text_r, sub_text_g, sub_text_b) = if is_dark { (180, 180, 190) } else { (100, 100, 110) };
        let (line_r, line_g, line_b) = if is_dark { (60, 60, 65) } else { (200, 200, 200) };

        compositor.draw_rect(x, y, width, height, bg_r, bg_g, bg_b);
        
        // Sidebar
        let sidebar_width = 120;
        let content_width = width.saturating_sub(sidebar_width);
        compositor.draw_rect(x, y, sidebar_width, height, sb_r, sb_g, sb_b);
        draw_string(compositor, x + 10, y + 10, "Favorites", sub_text_r, sub_text_g, sub_text_b);
        draw_string(compositor, x + 10, y + 30, " Desktop", text_r, text_g, text_b);
        draw_string(compositor, x + 10, y + 50, " Documents", text_r, text_g, text_b);
        
        // Toolbar
        let toolbar_height = 40;
        compositor.draw_rect(x + sidebar_width, y, content_width, toolbar_height, tb_r, tb_g, tb_b);
        draw_string(compositor, x + sidebar_width + 10, y + 15, "New File: Type name & press Enter", sub_text_r, sub_text_g, sub_text_b);
        
        // Separator lines
        compositor.draw_rect(x + sidebar_width - 1, y, 1, height, line_r, line_g, line_b); 
        compositor.draw_rect(x + sidebar_width, y + toolbar_height - 1, content_width, 1, line_r, line_g, line_b);
        
        let mut item_count = self.items.len();
        if self.is_creating_file || !self.new_file_name.is_empty() {
            item_count += 1;
        }

        let total_content_height = item_count * 30 + 10;
        let available_height = height.saturating_sub(toolbar_height);
        let max_scroll = (total_content_height as isize - available_height as isize).max(0);

        self.scroll_y = self.scroll_y.clamp(0, max_scroll);
        // Removed scrollbar space deduction to render as overlay
        let content_x = x + sidebar_width + 10;
        let mut content_y = (y + toolbar_height + 10) as isize - self.scroll_y;
        
        for (name, is_dir) in &self.items {
            if content_y > (y + height) as isize { break; } 
            
            if content_y >= (y + toolbar_height) as isize {
                if !is_dir {
                    // Document icon
                    compositor.draw_rect(content_x, content_y as usize, 16, 20, 255, 255, 255);
                    compositor.draw_rect(content_x, content_y as usize, 16, 1, 150, 150, 150);
                    compositor.draw_rect(content_x, content_y as usize, 1, 20, 150, 150, 150);
                    compositor.draw_rect(content_x + 15, content_y as usize, 1, 20, 150, 150, 150);
                    compositor.draw_rect(content_x, content_y as usize + 19, 16, 1, 150, 150, 150);
                } else {
                    // Folder icon
                    compositor.draw_rect(content_x, content_y as usize + 4, 20, 14, 100, 180, 255);
                    compositor.draw_rect(content_x, content_y as usize, 10, 4, 100, 180, 255);
                }
                
                let max_chars = if content_width > 40 { (content_width - 40) / 8 } else { 0 };
                let display_name = if name.chars().count() > max_chars && max_chars > 3 {
                    alloc::format!("{}...", &name[..max_chars - 3])
                } else {
                    name.clone()
                };
                draw_string(compositor, content_x + 30, content_y as usize + 4, &display_name, text_r, text_g, text_b);
            }
            content_y += 30;
        }

        // Show typing indicator if creating file
        if (self.is_creating_file || !self.new_file_name.is_empty()) && content_y <= (y + height) as isize && content_y >= (y + toolbar_height) as isize {
            compositor.draw_rect(content_x, content_y as usize, 16, 20, 255, 255, 255); // Doc icon
            draw_string(compositor, content_x + 30, content_y as usize + 4, &self.new_file_name, 0, 120, 255);
            draw_string(compositor, content_x + 30 + self.new_file_name.len() * 8, content_y as usize + 4, "_", 0, 120, 255);
        }

        compositor.draw_scrollbar(x + width - 10, y + toolbar_height, available_height, self.scroll_y as usize, max_scroll as usize);
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::KeyPress(c) => {
                if c == '\n' {
                    if !self.new_file_name.is_empty() {
                        let _ = crate::fs::RAM_FS.write_file(&self.new_file_name, b"");
                        self.new_file_name.clear();
                        self.is_creating_file = false;
                        self.update(); // UI nach Erstellung erneuern
                    }
                } else if c == '\x08' {
                    let _ = self.new_file_name.pop();
                } else if self.new_file_name.len() < 30 {
                    self.new_file_name.push(c);
                    self.is_creating_file = true;
                }
            }
            Event::MouseLongPress { x, y } => {
                let sidebar_width = 120;
                let toolbar_height = 40;
                // Prüfe ob Klick im Bereich der Dateiliste war
                if x > sidebar_width && y > toolbar_height + 10 {
                    let item_y_offset = y.saturating_sub(toolbar_height + 10) as isize + self.scroll_y;
                    if item_y_offset >= 0 {
                        let idx = (item_y_offset as usize) / 30; // 30 Pixel Höhe pro Datei-Element
                        if idx < self.items.len() {
                            let (name, is_dir) = &self.items[idx];
                            if !*is_dir {
                                let _ = crate::fs::RAM_FS.delete_file(name);
                                self.update();
                            }
                        }
                    }
                }
            }
            Event::MouseScroll { delta } => {
                self.scroll_y += (delta as isize) * 30; // 30 is item height
                if self.scroll_y < 0 { self.scroll_y = 0; }
            }
            _ => {}
        }
    }
}
