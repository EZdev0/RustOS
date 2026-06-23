use crate::desktop::app::{App, Event};
use crate::desktop::compositor::GraphicalCompositor;
use alloc::string::String;
use alloc::vec::Vec;

fn draw_string(compositor: &mut GraphicalCompositor, mut x: usize, y: usize, s: &str, r: u8, g: u8, b: u8) {
    for c in s.chars() {
        compositor.draw_char(x, y, c, r, g, b);
        x += 8;
    }
}

pub struct FileManagerApp {
    files: Vec<String>,
    new_file_name: String,
    is_creating_file: bool,
}

impl FileManagerApp {
    pub fn new() -> Self {
        Self {
            files: crate::fs::RAM_FS.lock().list_files(),
            new_file_name: String::new(),
            is_creating_file: false,
        }
    }
}

impl App for FileManagerApp {
    fn title(&self) -> &str {
        "Finder"
    }

    fn update(&mut self) {
        self.files = crate::fs::RAM_FS.lock().list_files();
    }

    fn draw(&mut self, compositor: &mut GraphicalCompositor, x: usize, y: usize, width: usize, height: usize) {
        // MacOS-like background
        compositor.draw_rect(x, y, width, height, 240, 240, 245);
        
        // Sidebar
        let sidebar_width = 120;
        compositor.draw_rect(x, y, sidebar_width, height, 220, 220, 230);
        draw_string(compositor, x + 10, y + 10, "Favorites", 100, 100, 110);
        draw_string(compositor, x + 10, y + 30, " Desktop", 50, 50, 60);
        draw_string(compositor, x + 10, y + 50, " Documents", 50, 50, 60);
        
        // Toolbar
        let toolbar_height = 40;
        compositor.draw_rect(x + sidebar_width, y, width - sidebar_width, toolbar_height, 230, 230, 240);
        draw_string(compositor, x + sidebar_width + 10, y + 15, "New File: Type name & press Enter", 80, 80, 90);
        
        // Separator lines
        compositor.draw_rect(x + sidebar_width - 1, y, 1, height, 200, 200, 200); 
        compositor.draw_rect(x + sidebar_width, y + toolbar_height - 1, width - sidebar_width, 1, 200, 200, 200);
        
        // Content area - File list
        let content_x = x + sidebar_width + 10;
        let mut content_y = y + toolbar_height + 10;
        
        for file in &self.files {
            if content_y + 20 > y + height { break; } 
            
            if file.contains(".") {
                // Document icon
                compositor.draw_rect(content_x, content_y, 16, 20, 255, 255, 255);
                compositor.draw_rect(content_x, content_y, 16, 1, 150, 150, 150);
                compositor.draw_rect(content_x, content_y, 1, 20, 150, 150, 150);
                compositor.draw_rect(content_x + 15, content_y, 1, 20, 150, 150, 150);
                compositor.draw_rect(content_x, content_y + 19, 16, 1, 150, 150, 150);
            } else {
                // Folder icon
                compositor.draw_rect(content_x, content_y + 4, 20, 14, 100, 180, 255);
                compositor.draw_rect(content_x, content_y, 10, 4, 100, 180, 255);
            }
            
            draw_string(compositor, content_x + 30, content_y + 4, file, 30, 30, 40);
            content_y += 30;
        }

        // Show typing indicator if creating file
        if self.is_creating_file || self.new_file_name.len() > 0 {
            if content_y + 20 <= y + height {
                compositor.draw_rect(content_x, content_y, 16, 20, 255, 255, 255); // Doc icon
                draw_string(compositor, content_x + 30, content_y + 4, &self.new_file_name, 0, 0, 255);
                draw_string(compositor, content_x + 30 + self.new_file_name.len() * 8, content_y + 4, "_", 0, 0, 255);
            }
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::KeyPress(c) => {
                if c == '\n' {
                    if !self.new_file_name.is_empty() {
                        crate::fs::RAM_FS.lock().write_file(&self.new_file_name, b"");
                        self.new_file_name.clear();
                        self.is_creating_file = false;
                    }
                } else if c == '\x08' {
                    let _ = self.new_file_name.pop();
                } else {
                    self.new_file_name.push(c);
                    self.is_creating_file = true;
                }
            }
            _ => {}
        }
    }
}
